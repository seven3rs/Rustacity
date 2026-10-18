// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! The extension manager, as a program.
//!
//! Run it, and it says where the extensions are, starts the local server, and
//! opens it in the browser.  The interface is optional - the API answers either
//! way, and `--print-root` is the whole tool when all you want is the path.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use rustacity_ext_manager::{open, paths, server};

/// What the manager is and what it takes.
const USAGE: &str = "\
Usage:
rustacity-ext-manager [options]

Lists the extensions installed for Rustacity, turns them on and off, and asks
the extension host whether each one still loads.  It starts a server on the
loopback interface and opens the interface in a browser; a browser cannot touch
a directory itself, which is why there are two halves.

Extensions live in <user data directory>/Extensions, beside the editor's
Plug-Ins directory: %APPDATA%\\rustacity on Windows, ~/.rustacity-data on GTK,
~/Library/Application Support/rustacity on macOS.

Options:
--root <dir>       the extensions directory to manage; RUSTACITY_EXTENSIONS_DIR
                   sets the same thing, and the platform default is used if
                   neither does
--assets <dir>     the built interface to serve; by default a directory named
                   assets beside this program, if there is one
--port <n>         the port to listen on; 0 (the default) picks a free one
--no-open          do not open a browser
--print-root       print the extensions directory and stop
--help, -h         this text
--version          this tool's version and the extension API it speaks
";

/// How the manager failed: badly enough to explain the arguments, or just
/// badly.
#[derive(Debug)]
enum Fault {
    Usage(String),
    Failed(String),
}

/// What the arguments asked for.
enum Request {
    Run(Options),
    PrintRoot(PathBuf),
    Help,
    Version,
}

struct Options {
    root: PathBuf,
    assets: Option<PathBuf>,
    port: u16,
    open: bool,
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match run(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Fault::Usage(message)) => {
            eprintln!("rustacity-ext-manager: {message}");
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
        Err(Fault::Failed(message)) => {
            eprintln!("rustacity-ext-manager: {message}");
            ExitCode::from(1)
        }
    }
}

fn run(arguments: &[String]) -> Result<(), Fault> {
    match parse(arguments)? {
        Request::Help => {
            print!("{USAGE}");
            Ok(())
        }
        Request::Version => {
            println!(
                "rustacity-ext-manager {} (extension API {})",
                env!("CARGO_PKG_VERSION"),
                rustacity_ext_api::abi::API_VERSION
            );
            Ok(())
        }
        Request::PrintRoot(root) => {
            println!("{}", root.display());
            Ok(())
        }
        Request::Run(options) => serve(options),
    }
}

fn parse(arguments: &[String]) -> Result<Request, Fault> {
    let mut options = Options {
        root: PathBuf::new(),
        assets: None,
        port: 0,
        open: true,
    };
    let mut root: Option<PathBuf> = None;
    let mut print_root = false;

    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        index += 1;
        // Everything but the help and the version takes a value; taking it is
        // the same three lines every time, so it is written once here.
        let mut value = |what: &str| -> Result<String, Fault> {
            let value = arguments
                .get(index)
                .ok_or_else(|| Fault::Usage(format!("{what} needs a value after it")))?;
            index += 1;
            Ok(value.clone())
        };
        match argument {
            "--help" | "-h" => return Ok(Request::Help),
            "--version" => return Ok(Request::Version),
            "--no-open" => options.open = false,
            "--print-root" => print_root = true,
            "--root" => root = Some(PathBuf::from(value("--root")?)),
            "--assets" => options.assets = Some(PathBuf::from(value("--assets")?)),
            "--port" => {
                let text = value("--port")?;
                options.port = text
                    .parse::<u16>()
                    .map_err(|_| Fault::Usage(format!("'{text}' is not a port number")))?;
            }
            other => return Err(Fault::Usage(format!("'{other}' is not an option"))),
        }
    }

    let root = match root {
        Some(root) => root,
        None => paths::default_root().ok_or_else(|| {
            Fault::Failed(format!(
                "no extensions directory could be worked out; pass --root, or set {}",
                paths::ROOT_ENV
            ))
        })?,
    };
    options.root = root.clone();
    if print_root {
        return Ok(Request::PrintRoot(root));
    }
    if options.assets.is_none() {
        options.assets = default_assets();
    }
    Ok(Request::Run(options))
}

/// The interface beside this program, when it was built there.
fn default_assets() -> Option<PathBuf> {
    let beside = std::env::current_exe().ok()?.parent()?.join("assets");
    beside.is_dir().then_some(beside)
}

fn serve(options: Options) -> Result<(), Fault> {
    let listener = TcpListener::bind(("127.0.0.1", options.port)).map_err(|error| {
        Fault::Failed(format!(
            "cannot listen on 127.0.0.1:{}: {error}",
            options.port
        ))
    })?;
    let address = listener
        .local_addr()
        .map_err(|error| Fault::Failed(format!("cannot read back the address: {error}")))?;
    let url = format!("http://{address}/");

    println!("extensions: {}", options.root.display());
    println!("manager:    {url}");
    match &options.assets {
        Some(assets) => println!("interface:  {}", assets.display()),
        None => {
            println!("interface:  not built; run scripts/build-manager-ui.sh, or pass --assets")
        }
    }
    println!("api:        curl -H 'x-rustacity-manager: 1' {url}api/state");
    println!("stop with Ctrl-C");

    if options.open {
        if let Err(error) = open::open_url(&url) {
            eprintln!("could not open a browser: {error}; the manager is at {url}");
        }
    }

    let shared = Arc::new(server::Shared {
        root: options.root,
        assets: options.assets,
        changes: Mutex::new(()),
    });
    server::serve(listener, shared)
        .map_err(|error| Fault::Failed(format!("the server stopped: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(list: &[&str]) -> Vec<String> {
        list.iter().map(|argument| argument.to_string()).collect()
    }

    #[test]
    fn a_root_and_a_port_are_read() {
        let request = parse(&arguments(&[
            "--root",
            "/somewhere/extensions",
            "--port",
            "1234",
            "--no-open",
        ]))
        .expect("parsed");
        let Request::Run(options) = request else {
            panic!("not a run");
        };
        assert_eq!(options.root, PathBuf::from("/somewhere/extensions"));
        assert_eq!(options.port, 1234);
        assert!(!options.open);
    }

    #[test]
    fn the_root_can_be_printed_without_serving_anything() {
        let request = parse(&arguments(&["--root", "/somewhere", "--print-root"])).expect("parsed");
        let Request::PrintRoot(root) = request else {
            panic!("not a print");
        };
        assert_eq!(root, PathBuf::from("/somewhere"));
    }

    #[test]
    fn a_missing_value_is_a_usage_error() {
        assert!(matches!(
            parse(&arguments(&["--root"])),
            Err(Fault::Usage(_))
        ));
        assert!(matches!(
            parse(&arguments(&["--port"])),
            Err(Fault::Usage(_))
        ));
    }

    #[test]
    fn a_port_that_is_not_a_number_is_a_usage_error() {
        assert!(matches!(
            parse(&arguments(&["--port", "http"])),
            Err(Fault::Usage(_))
        ));
        assert!(matches!(
            parse(&arguments(&["--port", "70000"])),
            Err(Fault::Usage(_))
        ));
    }

    #[test]
    fn an_unknown_option_is_a_usage_error() {
        assert!(matches!(
            parse(&arguments(&["--verbose"])),
            Err(Fault::Usage(_))
        ));
    }
}
