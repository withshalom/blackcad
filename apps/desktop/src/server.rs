//! The loopback web server: the workbench's files, the page's heartbeat, and
//! the files the program was asked to open.
//!
//! Only 127.0.0.1 is bound. Every request must carry a `Host` of this
//! server's own address (a page reached through a rebound DNS name does not),
//! and every response is same-origin only. The calls the page makes to the
//! program carry a per-run token in a custom header, which a page from
//! another origin cannot send without a preflight this server never answers.

use std::borrow::Cow;
use std::collections::hash_map::RandomState;
use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasher, Hasher};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const OPEN_EXTENSIONS: [&str; 3] = ["dwg", "dxf", "blc"];
const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 64 * 1024;
/// The page polls every second; a minimised window's timers may slow to
/// once a minute, so "alive" is generous.
const ALIVE_WITHIN: Duration = Duration::from_secs(15);

pub trait Assets: Send + Sync {
    fn get(&self, path: &str) -> Option<Cow<'static, [u8]>>;
}

/// The files embedded at build time (empty in a development build).
pub struct Embedded;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/assets.rs"));
}

impl Embedded {
    pub fn is_empty() -> bool {
        generated::ASSETS.is_empty()
    }
}

impl Assets for Embedded {
    fn get(&self, path: &str) -> Option<Cow<'static, [u8]>> {
        generated::ASSETS.iter().find(|(p, _)| *p == path).map(|(_, b)| Cow::Borrowed(*b))
    }
}

/// A folder on disk (`--web-dir`), for working on the page without a build.
pub struct Folder(pub PathBuf);

impl Assets for Folder {
    fn get(&self, path: &str) -> Option<Cow<'static, [u8]>> {
        std::fs::read(self.0.join(path)).ok().map(Cow::Owned)
    }
}

#[derive(Default)]
struct Inner {
    last_poll: Option<Instant>,
    bye_at: Option<Instant>,
    pending: VecDeque<(String, String)>,
    files: HashMap<String, PathBuf>,
}

pub struct Shared {
    pub port: u16,
    /// Proves a caller is the page this run served.
    pub page_token: String,
    /// Proves a caller can read the instance file: a second launch.
    pub secret: String,
    inner: Mutex<Inner>,
}

/// 128 random bits as hex. `RandomState` is seeded by the operating system.
pub fn random_token() -> String {
    let mut out = String::new();
    for _ in 0..2 {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

impl Shared {
    pub fn new(port: u16, secret: String) -> Shared {
        Shared { port, page_token: random_token(), secret, inner: Mutex::new(Inner::default()) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Queue a drawing for the page to open. Only .dwg, .dxf and .blc files.
    pub fn add_file(&self, path: &Path) -> Result<(), String> {
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
        if !OPEN_EXTENSIONS.contains(&ext.as_str()) {
            return Err(format!("{}: not a .dwg, .dxf or .blc file", path.display()));
        }
        let path = std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !path.is_file() {
            return Err(format!("{}: not a file", path.display()));
        }
        let name: String = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
            .chars()
            .map(|c| if c.is_control() { '_' } else { c })
            .collect();
        let token = random_token();
        let mut g = self.lock();
        g.files.insert(token.clone(), path);
        g.pending.push_back((token, name));
        Ok(())
    }

    /// Has a window polled recently (and not said goodbye)?
    pub fn window_alive(&self) -> bool {
        let g = self.lock();
        match (g.last_poll, g.bye_at) {
            (Some(p), Some(b)) if p < b => false,
            (Some(p), _) => p.elapsed() < ALIVE_WITHIN,
            _ => false,
        }
    }

    /// Should the program end? Not before the first window has appeared
    /// (`grace`), then when the window said goodbye and nothing replaced it
    /// (a reload says goodbye too), or when it has been silent for ten
    /// minutes (a crash).
    pub fn should_exit(&self, started: Instant, grace: Duration) -> bool {
        let g = self.lock();
        match g.last_poll {
            None => started.elapsed() > grace,
            Some(p) => match g.bye_at {
                Some(b) if p < b => b.elapsed() > Duration::from_secs(6),
                _ => p.elapsed() > Duration::from_secs(600),
            },
        }
    }
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

fn read_request(s: &mut TcpStream) -> Result<Request, &'static str> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err("request head too large");
        }
        let n = s.read(&mut chunk).map_err(|_| "read failed")?;
        if n == 0 {
            return Err("closed");
        }
        buf.extend_from_slice(chunk.get(..n).unwrap_or_default());
    };
    let head = String::from_utf8_lossy(buf.get(..head_end).unwrap_or_default()).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split(' ');
    let method = first.next().unwrap_or_default().to_string();
    let target = first.next().unwrap_or_default();
    let path = target.split(['?', '#']).next().unwrap_or_default().to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    let mut req = Request { method, path, headers, body: Vec::new() };
    let len: usize = req.header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if len > MAX_BODY {
        return Err("body too large");
    }
    let mut body = buf.get(head_end + 4..).unwrap_or_default().to_vec();
    while body.len() < len {
        let n = s.read(&mut chunk).map_err(|_| "read failed")?;
        if n == 0 {
            return Err("closed");
        }
        body.extend_from_slice(chunk.get(..n).unwrap_or_default());
    }
    body.truncate(len);
    req.body = body;
    Ok(req)
}

fn respond(s: &mut TcpStream, status: &str, ctype: &str, body: &[u8], head_only: bool) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\n\
         X-Content-Type-Options: nosniff\r\nCross-Origin-Resource-Policy: same-origin\r\n\
         Referrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(head.as_bytes());
    if !head_only {
        let _ = s.write_all(body);
    }
    let _ = s.flush();
}

pub fn host_ok(host: Option<&str>, port: u16) -> bool {
    matches!(host, Some(h) if h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}"))
}

/// A path under the web folder, or None when it could leave it.
pub fn clean_path(path: &str) -> Option<String> {
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    if rel.contains(['\\', '%', '\0']) {
        return None;
    }
    let ok = Path::new(rel).components().all(|c| matches!(c, Component::Normal(_)));
    ok.then(|| rel.to_string())
}

pub fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default().to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "json" | "webmanifest" => "application/manifest+json",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

const PAGE_SCRIPT: &str = r#"(() => {
  const T = "@TOKEN@";
  const H = { "X-BlackCAD-Token": T };
  const call = (p) => fetch(p, { method: "POST", headers: H, keepalive: true });
  addEventListener("pagehide", (e) => { if (!e.persisted) call("/__desktop/bye").catch(() => {}); });
  async function drop(token, name) {
    const r = await fetch("/__file/" + token);
    if (!r.ok) return;
    const dt = new DataTransfer();
    dt.items.add(new File([await r.blob()], name));
    document.body.dispatchEvent(new DragEvent("drop", { dataTransfer: dt, bubbles: true, cancelable: true }));
  }
  async function poll() {
    try {
      const r = await call("/__desktop/poll");
      if (r.ok) {
        for (const line of (await r.text()).split("\n")) {
          const i = line.indexOf("\t");
          if (i > 0) await drop(line.slice(0, i), line.slice(i + 1));
        }
      }
    } catch (_) {}
    setTimeout(poll, 1000);
  }
  // Give the app time to set up its drop handler before the first file.
  const start = () => setTimeout(poll, 1500);
  if (document.readyState === "complete") start(); else addEventListener("load", start);
})();
"#;

fn inject(html: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(html);
    let tag = "<script src=\"/__desktop.js\"></script>";
    match text.rfind("</body>") {
        Some(i) => format!("{}{tag}{}", &text[..i], &text[i..]).into_bytes(),
        None => format!("{text}{tag}").into_bytes(),
    }
}

pub fn handle(mut s: TcpStream, shared: &Shared, assets: &dyn Assets) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let req = match read_request(&mut s) {
        Ok(r) => r,
        Err(_) => return respond(&mut s, "400 Bad Request", "text/plain", b"bad request", false),
    };
    if !host_ok(req.header("host"), shared.port) {
        return respond(&mut s, "421 Misdirected Request", "text/plain", b"wrong host", false);
    }
    let head_only = req.method == "HEAD";
    let token_ok = req.header("x-blackcad-token") == Some(shared.page_token.as_str());
    match (req.method.as_str(), req.path.as_str()) {
        ("POST", "/__desktop/poll") if token_ok => {
            let mut g = shared.lock();
            g.last_poll = Some(Instant::now());
            g.bye_at = None;
            let body: String = g.pending.drain(..).map(|(t, n)| format!("{t}\t{n}\n")).collect();
            drop(g);
            respond(&mut s, "200 OK", "text/plain; charset=utf-8", body.as_bytes(), false);
        }
        ("POST", "/__desktop/bye") if token_ok => {
            shared.lock().bye_at = Some(Instant::now());
            respond(&mut s, "204 No Content", "text/plain", b"", false);
        }
        ("POST", "/__desktop/open") if req.header("x-blackcad-secret") == Some(shared.secret.as_str()) => {
            let text = String::from_utf8_lossy(&req.body).into_owned();
            let mut failed = String::new();
            for line in text.lines().filter(|l| !l.is_empty()) {
                if let Err(e) = shared.add_file(Path::new(line)) {
                    failed.push_str(&e);
                    failed.push('\n');
                }
            }
            let alive = u8::from(shared.window_alive());
            respond(&mut s, "200 OK", "text/plain; charset=utf-8", format!("windows={alive}\n{failed}").as_bytes(), false);
        }
        ("POST", p) if p.starts_with("/__desktop/") => respond(&mut s, "403 Forbidden", "text/plain", b"forbidden", false),
        ("GET" | "HEAD", "/__desktop.js") => {
            let js = PAGE_SCRIPT.replace("@TOKEN@", &shared.page_token);
            respond(&mut s, "200 OK", mime("x.js"), js.as_bytes(), head_only);
        }
        ("GET" | "HEAD", p) if p.starts_with("/__file/") => {
            let token = p.trim_start_matches("/__file/");
            let path = shared.lock().files.get(token).cloned();
            match path.and_then(|p| std::fs::read(p).ok()) {
                Some(bytes) => respond(&mut s, "200 OK", "application/octet-stream", &bytes, head_only),
                None => respond(&mut s, "404 Not Found", "text/plain", b"not found", head_only),
            }
        }
        ("GET" | "HEAD", p) => match clean_path(p).and_then(|rel| assets.get(&rel).map(|b| (rel, b))) {
            Some((rel, bytes)) => {
                let body: Cow<[u8]> = if rel == "index.html" { Cow::Owned(inject(&bytes)) } else { bytes };
                respond(&mut s, "200 OK", mime(&rel), &body, head_only);
            }
            None => respond(&mut s, "404 Not Found", "text/plain", b"not found", head_only),
        },
        _ => respond(&mut s, "405 Method Not Allowed", "text/plain", b"method not allowed", false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::Arc;

    struct Fake;
    impl Assets for Fake {
        fn get(&self, path: &str) -> Option<Cow<'static, [u8]>> {
            match path {
                "index.html" => Some(Cow::Borrowed(b"<html><body>hi</body></html>")),
                "core.wasm" => Some(Cow::Borrowed(b"\0asm\x01\0\0\0")),
                _ => None,
            }
        }
    }

    fn start() -> (u16, Arc<Shared>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let shared = Arc::new(Shared::new(port, "sekrit".into()));
        let sh = shared.clone();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                handle(s, &sh, &Fake);
            }
        });
        (port, shared)
    }

    fn http(port: u16, req: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        String::from_utf8_lossy(&out).into_owned()
    }

    fn get(port: u16, path: &str, host: &str) -> String {
        http(port, &format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n"))
    }

    #[test]
    fn serves_index_with_script_and_wasm_type() {
        let (p, _) = start();
        let r = get(p, "/", &format!("127.0.0.1:{p}"));
        assert!(r.starts_with("HTTP/1.1 200"), "{r}");
        assert!(r.contains("<script src=\"/__desktop.js\"></script></body>"));
        let w = get(p, "/core.wasm", &format!("localhost:{p}"));
        assert!(w.contains("Content-Type: application/wasm"));
        assert!(get(p, "/nope", &format!("127.0.0.1:{p}")).starts_with("HTTP/1.1 404"));
    }

    #[test]
    fn rebound_hosts_and_traversal_are_refused() {
        let (p, _) = start();
        assert!(get(p, "/", "evil.example").starts_with("HTTP/1.1 421"));
        assert!(get(p, "/", &format!("evil.example:{p}")).starts_with("HTTP/1.1 421"));
        assert!(get(p, "/../secret", &format!("127.0.0.1:{p}")).starts_with("HTTP/1.1 404"));
        assert_eq!(clean_path("/a/../b"), None);
        assert_eq!(clean_path("/%2e%2e/x"), None);
        assert_eq!(clean_path("/a\\b"), None);
        assert_eq!(clean_path("/"), Some("index.html".into()));
        assert_eq!(clean_path("/icons/icon.svg"), Some("icons/icon.svg".into()));
    }

    #[test]
    fn page_calls_need_the_token() {
        let (p, sh) = start();
        let host = format!("127.0.0.1:{p}");
        let no = http(p, &format!("POST /__desktop/poll HTTP/1.1\r\nHost: {host}\r\n\r\n"));
        assert!(no.starts_with("HTTP/1.1 403"), "{no}");
        assert!(!sh.window_alive());
        let yes = http(
            p,
            &format!("POST /__desktop/poll HTTP/1.1\r\nHost: {host}\r\nX-BlackCAD-Token: {}\r\n\r\n", sh.page_token),
        );
        assert!(yes.starts_with("HTTP/1.1 200"));
        assert!(sh.window_alive());
        let js = get(p, "/__desktop.js", &host);
        assert!(js.contains(&sh.page_token));
    }

    #[test]
    fn opened_files_reach_the_page_once() {
        let (p, sh) = start();
        let host = format!("127.0.0.1:{p}");
        let dir = std::env::temp_dir().join(format!("bc-desktop-test-{}", random_token()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("part.dxf");
        std::fs::write(&f, b"0\nEOF\n").unwrap();
        let txt = dir.join("notes.txt");
        std::fs::write(&txt, b"x").unwrap();
        assert!(sh.add_file(&txt).is_err());
        let body = format!("{}\n{}\n", f.display(), txt.display());
        let r = http(
            p,
            &format!(
                "POST /__desktop/open HTTP/1.1\r\nHost: {host}\r\nX-BlackCAD-Secret: sekrit\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(r.contains("windows=0"), "{r}");
        assert!(r.contains("not a .dwg, .dxf or .blc"));
        let poll = format!("POST /__desktop/poll HTTP/1.1\r\nHost: {host}\r\nX-BlackCAD-Token: {}\r\n\r\n", sh.page_token);
        let first = http(p, &poll);
        let line = first.split("\r\n\r\n").nth(1).unwrap_or_default();
        let (token, name) = line.trim().split_once('\t').unwrap();
        assert_eq!(name, "part.dxf");
        assert!(http(p, &poll).ends_with("\r\n\r\n"), "drained");
        let file = get(p, &format!("/__file/{token}"), &host);
        assert!(file.ends_with("0\nEOF\n"));
        assert!(get(p, "/__file/0000", &host).starts_with("HTTP/1.1 404"));
        let bad = http(p, &format!("POST /__desktop/open HTTP/1.1\r\nHost: {host}\r\nX-BlackCAD-Secret: no\r\n\r\n"));
        assert!(bad.starts_with("HTTP/1.1 403"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn exit_rules() {
        let sh = Shared::new(1, "s".into());
        let t = Instant::now();
        assert!(!sh.should_exit(t, Duration::from_secs(60)));
        assert!(sh.should_exit(t, Duration::ZERO) || t.elapsed() == Duration::ZERO);
        {
            let mut g = sh.lock();
            g.last_poll = Some(Instant::now());
        }
        assert!(!sh.should_exit(t, Duration::ZERO));
        {
            let mut g = sh.lock();
            g.bye_at = Some(Instant::now() - Duration::from_secs(7));
            g.last_poll = Some(Instant::now() - Duration::from_secs(8));
        }
        assert!(sh.should_exit(t, Duration::from_secs(60)));
        assert!(!sh.window_alive());
    }

    #[test]
    fn tokens_differ() {
        assert_ne!(random_token(), random_token());
        assert_eq!(random_token().len(), 32);
    }
}
