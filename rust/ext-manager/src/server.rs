// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! The local server: the built interface, and the API it drives.
//!
//! The interface is a WebAssembly program in a browser, and a browser cannot
//! read or write a directory on its own.  This server is the part of the
//! manager that can, so the two halves are the same process's front end and
//! back end: it serves the built interface at `/`, and answers the small JSON
//! API under `/api/` that the interface calls.
//!
//! Three things it deliberately does not do:
//!
//! - it never listens on anything but the loopback interface;
//! - it never reads or writes outside the extensions root (the one exception is
//!   serving the interface's own files, which are addressed by a path that may
//!   not leave the directory it serves them from);
//! - it never answers a request to the API that does not carry the manager's
//!   own header.  That is not authentication - anything on the machine can send
//!   a header - but it means a page from somewhere else cannot drive the API
//!   with a form post, because the header forces a cross-origin caller through
//!   a preflight that is never answered.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use crate::model::{self, ManagerError, Slot};
use crate::verify;

/// How long a connection may take to say something, or hear something back.
const TIMEOUT: Duration = Duration::from_secs(10);
/// How large a request head may be.
const MAX_HEAD: usize = 16 * 1024;
/// How large a request body may be.  The API takes names, not files.
const MAX_BODY: usize = 64 * 1024;

/// The header every request to the API must carry, and its value.
const GUARD: (&str, &str) = ("x-rustacity-manager", "1");

/// What the server needs to answer anything.
pub struct Shared {
    /// The extensions directory.  Every change happens inside it.
    pub root: PathBuf,
    /// The built interface, when it has been built.
    pub assets: Option<PathBuf>,
    /// One change at a time: without this, two requests arriving together
    /// could both find the destination free and one of them would then fail at
    /// the rename with a message that makes no sense to the user.
    pub changes: Mutex<()>,
}

/// Answers connections until the listener fails.
///
/// One thread per connection, and connections are closed after one response:
/// this is a tool a person runs, not a server under load.
pub fn serve(listener: TcpListener, shared: Arc<Shared>) -> io::Result<()> {
    for stream in listener.incoming() {
        let stream = stream?;
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || {
            let _ = handle(stream, &shared);
        });
    }
    Ok(())
}

/// One request, as much of it as this server looks at.
struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header == name)
            .map(|(_, value)| value.as_str())
    }

    fn json<T: for<'de> Deserialize<'de>>(&self) -> Result<T, String> {
        serde_json::from_slice(&self.body)
            .map_err(|error| format!("the request body is not the JSON this API takes: {error}"))
    }
}

/// The body of a change: which extension, and where it is now.
#[derive(Deserialize)]
struct Change {
    name: String,
    #[serde(default)]
    slot: Option<Slot>,
}

fn handle(stream: TcpStream, shared: &Shared) -> io::Result<()> {
    let mut stream = stream;
    let _ = stream.set_read_timeout(Some(TIMEOUT));
    let _ = stream.set_write_timeout(Some(TIMEOUT));

    let request = match read_request(&mut stream) {
        Ok(Some(request)) => request,
        Ok(None) => return Ok(()),
        Err(error) => {
            let response = Response::text(
                400,
                "text/plain; charset=utf-8",
                format!("bad request: {error}\n"),
            );
            return response.write(&mut stream);
        }
    };
    route(&request, shared).write(&mut stream)
}

fn read_request(stream: &mut TcpStream) -> io::Result<Option<Request>> {
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(None);
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();
    if method.is_empty() || target.is_empty() {
        return Err(invalid("no request line"));
    }

    let mut head = request_line.len();
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        head += line.len();
        if head > MAX_HEAD {
            return Err(invalid("the request head is too large"));
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }

    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    if length > MAX_BODY {
        return Err(invalid("the request body is too large"));
    }
    let mut body = vec![0u8; length];
    if length > 0 {
        reader.read_exact(&mut body)?;
    }

    Ok(Some(Request {
        method,
        target,
        headers,
        body,
    }))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

fn route(request: &Request, shared: &Shared) -> Response {
    // A query or a fragment is the browser's business, not the router's.
    let target = request.target.split(['?', '#']).next().unwrap_or("/");

    if let Some(action) = target.strip_prefix("/api/") {
        if request.header(GUARD.0) != Some(GUARD.1) {
            return Response::json(
                403,
                json!({
                    "ok": false,
                    "error": "this request did not come from the manager's own page",
                }),
            );
        }
        return api(action, request, shared);
    }

    match request.method.as_str() {
        "GET" | "HEAD" => serve_asset(target, shared),
        _ => Response::text(
            405,
            "text/plain; charset=utf-8",
            "the manager serves only GET outside its API\n".to_owned(),
        ),
    }
}

fn api(action: &str, request: &Request, shared: &Shared) -> Response {
    match action {
        "state" => {
            let inventory = model::scan(&shared.root);
            Response::json(200, json!({ "ok": true, "inventory": inventory }))
        }
        "enable" | "disable" => {
            let change: Change = match request.json() {
                Ok(change) => change,
                Err(error) => return Response::bad(error),
            };
            let _one_at_a_time = shared.changes.lock();
            match model::set_enabled(&shared.root, &change.name, action == "enable") {
                Ok(outcome) => Response::json(200, json!({ "ok": true, "outcome": outcome })),
                Err(error) => Response::failed(&error),
            }
        }
        "verify" => {
            let change: Change = match request.json() {
                Ok(change) => change,
                Err(error) => return Response::bad(error),
            };
            match verify::verify(
                &shared.root,
                &change.name,
                change.slot.unwrap_or(Slot::Enabled),
            ) {
                Ok(report) => Response::json(200, json!({ "ok": true, "report": report })),
                Err(error) => Response::failed(&error),
            }
        }
        "reveal" => match crate::open::reveal(&shared.root) {
            Ok(()) => Response::json(200, json!({ "ok": true })),
            Err(error) => Response::json(
                500,
                json!({
                    "ok": false,
                    "error": format!("could not open {}: {error}", shared.root.display()),
                }),
            ),
        },
        _ => Response::json(
            404,
            json!({ "ok": false, "error": format!("no such action: {action}") }),
        ),
    }
}

fn serve_asset(target: &str, shared: &Shared) -> Response {
    let relative = target.trim_start_matches('/');
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };

    let Some(assets) = &shared.assets else {
        // Built to use whichever half is missing as the message.
        return Response::text(200, "text/html; charset=utf-8", not_built().to_owned());
    };

    let Some(path) = inside(assets, relative) else {
        return Response::text(
            404,
            "text/plain; charset=utf-8",
            "no such file\n".to_owned(),
        );
    };
    match std::fs::read(&path) {
        Ok(body) => Response::new(200, content_type(&path), body),
        Err(_) => Response::text(
            404,
            "text/plain; charset=utf-8",
            "no such file\n".to_owned(),
        ),
    }
}

/// Resolves `relative` under `root`, or nothing if it would leave it.
///
/// Only plain names and `./` are accepted, so `..`, an absolute path and a
/// Windows separator are all rejected rather than resolved.
fn inside(root: &Path, relative: &str) -> Option<PathBuf> {
    if relative.contains('\\') {
        return None;
    }
    let candidate = Path::new(relative);
    for component in candidate.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            _ => return None,
        }
    }
    let joined = root.join(candidate);
    joined.is_file().then_some(joined)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        // The one that matters: without it a browser refuses to compile the
        // module from the stream.
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

/// What `/` says when the interface has not been built.
fn not_built() -> &'static str {
    "<!DOCTYPE html>\n\
     <html lang=\"en\">\n\
     <head><meta charset=\"utf-8\"><title>Rustacity extension manager</title></head>\n\
     <body style=\"font-family: sans-serif; max-width: 40em; margin: 3em auto;\">\n\
     <h1>Rustacity extension manager</h1>\n\
     <p>The manager is running, but the page it serves has not been built yet.\n\
     Build it with <code>scripts/build-manager-ui.sh</code>, or start the manager\n\
     with <code>--assets</code> pointing at a built copy.</p>\n\
     <p>The API is running regardless: <a href=\"/api/state\">/api/state</a> lists\n\
     what is installed.</p>\n\
     </body>\n\
     </html>\n"
}

struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn new(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Response {
            status,
            content_type,
            body,
        }
    }

    fn text(status: u16, content_type: &'static str, body: String) -> Self {
        Response::new(status, content_type, body.into_bytes())
    }

    fn json(status: u16, value: serde_json::Value) -> Self {
        Response::text(
            status,
            "application/json; charset=utf-8",
            format!("{value}\n"),
        )
    }

    /// A mistake in the request, in the API's own words.
    fn bad(message: String) -> Self {
        Response::json(400, json!({ "ok": false, "error": message }))
    }

    /// A mistake in the directory, in the filesystem's own words.
    fn failed(error: &ManagerError) -> Self {
        Response::json(400, json!({ "ok": false, "error": error.to_string() }))
    }

    fn write(&self, stream: &mut TcpStream) -> io::Result<()> {
        let head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            self.status,
            reason(self.status),
            self.content_type,
            self.body.len()
        );
        stream.write_all(head.as_bytes())?;
        stream.write_all(&self.body)?;
        stream.flush()
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}
