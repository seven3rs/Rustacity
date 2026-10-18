// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! The manager's server, driven over a socket the way the browser drives it.
//!
//! Every test here starts a real server on a port of the operating system's
//! choosing and talks to it with a real socket, because the parts worth testing
//! are the parts a unit test cannot reach: that the API is what the interface
//! expects, that a request without the manager's header is refused, and that a
//! path in a request cannot leave the directory being served.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rustacity_ext_api::Manifest;
use rustacity_ext_manager::server::{self, Shared};

/// A working directory for one test, removed when the test ends.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rustacity-manager-server-{tag}-{}-{unique}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("fixture root");
        Fixture { root }
    }

    /// Installs an extension that names itself properly but has no module.
    fn install_manifest_only(&self, name: &str, id: &str) -> PathBuf {
        let directory = self.root.join(name);
        std::fs::create_dir_all(&directory).expect("extension directory");
        std::fs::write(
            Manifest::path_in(&directory),
            format!(
                "api_version = 1\nid = \"{id}\"\nname = \"{name}\"\nversion = \"0.1.0\"\ncapabilities = [\"audio.read\", \"audio.write\"]\n"
            ),
        )
        .expect("manifest");
        directory
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A server on a free port, which stops when the test ends.
struct Running {
    address: SocketAddr,
}

impl Running {
    fn start(root: &Path, assets: Option<PathBuf>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
        let address = listener.local_addr().expect("the address");
        let shared = Arc::new(Shared {
            root: root.to_path_buf(),
            assets,
            changes: Mutex::new(()),
        });
        std::thread::spawn(move || {
            let _ = server::serve(listener, shared);
        });
        Running { address }
    }

    /// One request, one connection, closed after the response.
    fn request(&self, method: &str, target: &str, guard: bool, body: &str) -> Reply {
        let mut stream = TcpStream::connect(self.address).expect("connect");
        let mut request = format!("{method} {target} HTTP/1.1\r\nHost: localhost\r\n");
        if guard {
            request.push_str("x-rustacity-manager: 1\r\n");
        }
        if !body.is_empty() {
            request.push_str("Content-Type: application/json\r\n");
            request.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        request.push_str("Connection: close\r\n\r\n");
        request.push_str(body);
        stream.write_all(request.as_bytes()).expect("write");
        stream.flush().expect("flush");

        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("read");
        let text = String::from_utf8_lossy(&response).to_string();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse::<u16>().ok())
            .unwrap_or(0);
        Reply {
            status,
            body: body.to_owned(),
        }
    }

    fn state(&self) -> String {
        let reply = self.request("GET", "/api/state", true, "");
        assert_eq!(reply.status, 200, "{}", reply.body);
        reply.body
    }
}

struct Reply {
    status: u16,
    body: String,
}

#[test]
fn the_state_lists_what_is_installed() {
    let fixture = Fixture::new("state");
    fixture.install_manifest_only("gain-invert", "com.rustacity.gain-invert");
    let running = Running::start(&fixture.root, None);

    let body = running.state();
    assert!(body.contains("com.rustacity.gain-invert"), "{body}");
    assert!(body.contains("\"slot\":\"enabled\""), "{body}");
    // The root is reported as the server was told about it.
    assert!(body.contains("gain-invert"), "{body}");
}

#[test]
fn the_api_will_not_answer_a_request_that_is_not_ours() {
    let fixture = Fixture::new("guard");
    let running = Running::start(&fixture.root, None);

    let reply = running.request("GET", "/api/state", false, "");
    assert_eq!(reply.status, 403, "{}", reply.body);
    assert!(reply.body.contains("did not come from"), "{}", reply.body);
}

#[test]
fn disabling_moves_the_extension_and_enabling_moves_it_back() {
    let fixture = Fixture::new("toggle");
    let directory = fixture.install_manifest_only("gain-invert", "com.rustacity.gain-invert");
    let running = Running::start(&fixture.root, None);

    let reply = running.request("POST", "/api/disable", true, r#"{ "name": "gain-invert" }"#);
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert!(!directory.exists());
    assert!(fixture.root.join("disabled").join("gain-invert").is_dir());
    assert!(running.state().contains("\"slot\":\"disabled\""));

    let reply = running.request("POST", "/api/enable", true, r#"{ "name": "gain-invert" }"#);
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert!(directory.is_dir());
    assert!(running.state().contains("\"slot\":\"enabled\""));
}

#[test]
fn a_name_that_would_leave_the_root_is_refused() {
    let fixture = Fixture::new("escape");
    fixture.install_manifest_only("gain-invert", "com.rustacity.gain-invert");
    let running = Running::start(&fixture.root, None);

    let reply = running.request("POST", "/api/disable", true, r#"{ "name": "../../etc" }"#);
    assert_eq!(reply.status, 400, "{}", reply.body);
    assert!(reply.body.contains("not a name"), "{}", reply.body);
}

#[test]
fn a_request_that_is_not_json_is_reported_as_such() {
    let fixture = Fixture::new("badjson");
    let running = Running::start(&fixture.root, None);

    let reply = running.request("POST", "/api/disable", true, "name=gain-invert");
    assert_eq!(reply.status, 400, "{}", reply.body);
    assert!(reply.body.contains("not the JSON"), "{}", reply.body);
}

#[test]
fn an_unknown_action_is_a_404_with_a_reason() {
    let fixture = Fixture::new("unknown");
    let running = Running::start(&fixture.root, None);

    let reply = running.request("GET", "/api/nonsense", true, "");
    assert_eq!(reply.status, 404, "{}", reply.body);
    assert!(reply.body.contains("no such action"), "{}", reply.body);
}

#[test]
fn verification_asks_the_host_and_reports_what_it_said() {
    let fixture = Fixture::new("verify");
    fixture.install_manifest_only("half-made", "com.rustacity.half-made");
    let running = Running::start(&fixture.root, None);

    let reply = running.request(
        "POST",
        "/api/verify",
        true,
        r#"{ "name": "half-made", "slot": "enabled" }"#,
    );
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert!(reply.body.contains("\"report\""), "{}", reply.body);
    // The report is the host's, and the host says no: there is no module to
    // load, so nothing could be run either.
    assert!(reply.body.contains("\"loaded\":false"), "{}", reply.body);
    assert!(reply.body.contains("\"ran\":false"), "{}", reply.body);
}

#[test]
fn the_page_says_so_when_the_interface_was_never_built() {
    let fixture = Fixture::new("no-assets");
    let running = Running::start(&fixture.root, None);

    let reply = running.request("GET", "/", true, "");
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert!(reply.body.contains("has not been built"), "{}", reply.body);
}

#[test]
fn a_served_path_cannot_leave_the_directory_being_served() {
    let fixture = Fixture::new("assets");
    let assets = fixture.root.join("assets");
    std::fs::create_dir_all(&assets).expect("assets");
    std::fs::write(assets.join("index.html"), "<!doctype html>hello").expect("index");
    // A file one level up, which is what a traversal would be looking for.
    std::fs::write(fixture.root.join("secret.txt"), "not for the browser").expect("secret");
    let running = Running::start(&fixture.root, Some(assets));

    let reply = running.request("GET", "/", true, "");
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert!(reply.body.contains("hello"), "{}", reply.body);

    for target in ["/../secret.txt", "/..%2fsecret.txt", "/./../secret.txt"] {
        let reply = running.request("GET", target, true, "");
        assert_eq!(reply.status, 404, "{target}: {}", reply.body);
        assert!(!reply.body.contains("not for the browser"), "{target}");
    }

    // And a file that is simply not there is a 404, not a 500.
    let reply = running.request("GET", "/missing.js", true, "");
    assert_eq!(reply.status, 404, "{}", reply.body);
}

#[test]
fn a_static_file_is_served_with_the_type_a_browser_needs() {
    let fixture = Fixture::new("content-type");
    let assets = fixture.root.join("assets");
    std::fs::create_dir_all(&assets).expect("assets");
    std::fs::write(assets.join("app.js"), "export default 1;").expect("js");
    std::fs::write(assets.join("app_bg.wasm"), b"\0asm").expect("wasm");
    let running = Running::start(&fixture.root, Some(assets));

    let js = running.request("GET", "/app.js", true, "");
    assert_eq!(js.status, 200);
    let wasm = running.request("GET", "/app_bg.wasm", true, "");
    assert_eq!(wasm.status, 200);
    // The body of the wasm reply is binary and lossily decoded above, so only
    // the header is checked here - which is the part that decides whether a
    // browser will compile it.
    let raw = fetch_head(&running, "/app_bg.wasm");
    assert!(raw.contains("application/wasm"), "{raw}");
}

/// The response head, for the tests that care about a header rather than a body.
fn fetch_head(running: &Running, target: &str) -> String {
    let mut stream = TcpStream::connect(running.address).expect("connect");
    stream
        .write_all(
            format!("GET {target} HTTP/1.1\r\nHost: localhost\r\nx-rustacity-manager: 1\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .expect("write");
    let mut response = Vec::new();
    stream.read_to_end(&mut response).expect("read");
    let text = String::from_utf8_lossy(&response).to_string();
    text.split("\r\n\r\n").next().unwrap_or("").to_owned()
}
