//! BlackCAD for the desktop.
//!
//! One small program with the browser workbench (`apps/web` plus the core as
//! WebAssembly, built by `cargo xtask web`) inside it. It serves the workbench
//! on 127.0.0.1, opens it in an app window of the Chromium based browser on
//! the computer, hands drawings given on the command line (or from a file
//! association) to the window, and ends when the window is closed. Drawings
//! never leave the computer.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod instance;
mod launch;
mod server;

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const NAME: &str = "BlackCAD";
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Tried first, so the page keeps one origin (and the browser keeps its
/// settings and storage for it) from run to run.
const PREFERRED_PORT: u16 = 47_631;
/// How long to wait for the window to appear before giving up.
const WINDOW_GRACE: Duration = Duration::from_secs(90);

const HELP: &str = "BlackCAD desktop

Usage: blackcad [OPTIONS] [FILE.dwg|FILE.dxf|FILE.blc ...]

Options:
  --web-dir DIR   serve the workbench from DIR instead of the built-in copy
  --port N        port to listen on (default 47631, else any free port)
  --no-window     serve only; print the address and wait
  --print-url     print the address of the running or started program
  -V, --version   print the version
  -h, --help      print this

Environment:
  BLACKCAD_BROWSER   path of the Chromium-based browser to use
  BLACKCAD_WEB_DIR   same as --web-dir
";

#[derive(Default)]
struct Args {
    files: Vec<PathBuf>,
    web_dir: Option<PathBuf>,
    port: Option<u16>,
    no_window: bool,
    print_url: bool,
    help: bool,
    version: bool,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = args;
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => a.help = true,
            "-V" | "--version" => a.version = true,
            "--no-window" => a.no_window = true,
            "--print-url" => a.print_url = true,
            "--web-dir" => a.web_dir = Some(it.next().ok_or("--web-dir needs a folder")?.into()),
            "--port" => {
                let v = it.next().ok_or("--port needs a number")?;
                a.port = Some(v.parse().map_err(|_| format!("--port: {v:?} is not a port number"))?);
            }
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            // macOS passes a process serial number when launched from the Finder.
            s if s.starts_with("-psn_") => {}
            _ => a.files.push(arg.into()),
        }
    }
    Ok(a)
}

fn log(data: &std::path::Path, msg: &str) {
    use std::io::Write;
    let _ = std::fs::create_dir_all(data);
    let path = data.join("desktop.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 256 * 1024) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{msg}");
    }
}

fn main() {
    let data = instance::data_dir();
    let code = match run(&data) {
        Ok(()) => 0,
        Err(e) => {
            log(&data, &format!("error: {e}"));
            eprintln!("{NAME}: {e}");
            1
        }
    };
    std::process::exit(code);
}

fn run(data: &std::path::Path) -> Result<(), String> {
    let args = parse_args(std::env::args().skip(1))?;
    if args.help {
        print!("{HELP}");
        return Ok(());
    }
    if args.version {
        println!("{NAME} {VERSION}");
        return Ok(());
    }
    let files: Vec<PathBuf> = args.files.iter().map(|f| std::fs::canonicalize(f).unwrap_or_else(|_| f.clone())).collect();
    let web_dir = args.web_dir.clone().or_else(|| std::env::var_os("BLACKCAD_WEB_DIR").map(PathBuf::from));

    // A copy is already running: give it the files (and a window if it has none).
    if web_dir.is_none() && args.port.is_none() {
        if let Some((port, window_open)) = instance::hand_off(data, &files) {
            let url = format!("http://127.0.0.1:{port}/");
            if args.print_url {
                println!("{url}");
            }
            if !window_open && !args.no_window {
                launch::open_window(&url, &instance::profile_dir(data))?;
            }
            return Ok(());
        }
    }

    let assets: Box<dyn server::Assets> = match &web_dir {
        Some(d) => Box::new(server::Folder(d.clone())),
        None if server::Embedded::is_empty() => {
            return Err("this build has no workbench inside it; run with --web-dir DIR (a folder made by `cargo xtask web`)".into());
        }
        None => Box::new(server::Embedded),
    };
    let listener = match args.port {
        Some(p) => TcpListener::bind(("127.0.0.1", p)),
        None => TcpListener::bind(("127.0.0.1", PREFERRED_PORT)).or_else(|_| TcpListener::bind(("127.0.0.1", 0))),
    }
    .map_err(|e| format!("cannot listen on 127.0.0.1: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let shared = Arc::new(server::Shared::new(port, server::random_token()));
    for f in &files {
        if let Err(e) = shared.add_file(f) {
            log(data, &e);
            eprintln!("{NAME}: {e}");
        }
    }
    let assets: Arc<dyn server::Assets> = Arc::from(assets);
    {
        let shared = shared.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (shared, assets) = (shared.clone(), assets.clone());
                std::thread::spawn(move || server::handle(stream, &shared, &*assets));
            }
        });
    }
    let started = Instant::now();
    if web_dir.is_none() && args.port.is_none() {
        instance::write(data, port, &shared.secret)?;
    }
    let url = format!("http://127.0.0.1:{port}/");
    if args.print_url || args.no_window {
        println!("{url}");
    }
    if !args.no_window {
        let used = launch::open_window(&url, &instance::profile_dir(data))?;
        log(data, &format!("{NAME} {VERSION} at {url} in {used}"));
    }
    while args.no_window || !shared.should_exit(started, WINDOW_GRACE) {
        std::thread::sleep(Duration::from_millis(500));
    }
    instance::remove(data, &shared.secret);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(v: &[&str]) -> Result<Args, String> {
        parse_args(v.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_files_and_options() {
        let a = p(&["a.dwg", "--port", "9", "--no-window", "b.dxf"]).unwrap();
        assert_eq!(a.files.len(), 2);
        assert_eq!(a.port, Some(9));
        assert!(a.no_window);
        assert!(p(&["--port", "x"]).is_err());
        assert!(p(&["--nope"]).is_err());
        assert!(p(&["-psn_0_1234"]).unwrap().files.is_empty());
    }
}
