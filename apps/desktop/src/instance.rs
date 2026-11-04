//! Where the program keeps its files, and the single-instance handshake.
//!
//! The first run writes `instance` (port and secret) in the data folder. A
//! later run reads it and hands its files to the first run over HTTP; if the
//! first run is gone the file is stale and the later run takes over.

use std::env;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    let base = env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = env::var_os("HOME").map(|h| PathBuf::from(h).join("Library").join("Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")));
    let name = if cfg!(target_os = "linux") { "blackcad" } else { "BlackCAD" };
    base.unwrap_or_else(env::temp_dir).join(name)
}

pub fn profile_dir(data: &Path) -> PathBuf {
    data.join("profile")
}

fn instance_file(data: &Path) -> PathBuf {
    data.join("instance")
}

pub fn write(data: &Path, port: u16, secret: &str) -> Result<(), String> {
    std::fs::create_dir_all(data).map_err(|e| format!("{}: {e}", data.display()))?;
    std::fs::write(instance_file(data), format!("{port}\n{secret}\n")).map_err(|e| e.to_string())
}

pub fn read(data: &Path) -> Option<(u16, String)> {
    let text = std::fs::read_to_string(instance_file(data)).ok()?;
    let mut l = text.lines();
    Some((l.next()?.trim().parse().ok()?, l.next()?.trim().to_string()))
}

/// Remove the file, but only if it is still ours.
pub fn remove(data: &Path, secret: &str) {
    if read(data).is_some_and(|(_, s)| s == secret) {
        let _ = std::fs::remove_file(instance_file(data));
    }
}

/// Hand `files` to a running instance. `Some(window_open)` when one took
/// them; `None` when none answered.
pub fn hand_off(data: &Path, files: &[PathBuf]) -> Option<(u16, bool)> {
    let (port, secret) = read(data)?;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(800)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let body: String = files.iter().map(|f| format!("{}\n", f.display())).collect();
    let req = format!(
        "POST /__desktop/open HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-BlackCAD-Secret: {secret}\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    if !out.starts_with("HTTP/1.1 200") {
        return None;
    }
    Some((port, out.contains("windows=1")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_file_round_trips_and_is_removed_only_by_its_owner() {
        let d = env::temp_dir().join(format!("bc-inst-{}", crate::server::random_token()));
        write(&d, 4242, "abc").unwrap();
        assert_eq!(read(&d), Some((4242, "abc".to_string())));
        remove(&d, "other");
        assert!(read(&d).is_some());
        remove(&d, "abc");
        assert!(read(&d).is_none());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn no_instance_means_no_hand_off() {
        let d = env::temp_dir().join(format!("bc-inst-{}", crate::server::random_token()));
        assert!(hand_off(&d, &[]).is_none());
    }
}
