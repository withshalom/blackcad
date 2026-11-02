//! Finding a Chromium-based browser and opening the workbench in an app
//! window of its own (no tabs, no address bar), with a profile folder that
//! belongs to BlackCAD so the user's own browsing is not touched.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The browser to use: `$BLACKCAD_BROWSER`, else the first one found.
pub fn find_browser() -> Option<PathBuf> {
    if let Some(p) = env::var_os("BLACKCAD_BROWSER") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    candidates().into_iter().find(|p| p.is_file())
}

#[cfg(target_os = "windows")]
fn candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    let roots = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"];
    let rels = [
        r"Google\Chrome\Application\chrome.exe",
        r"Microsoft\Edge\Application\msedge.exe",
        r"BraveSoftware\Brave-Browser\Application\brave.exe",
        r"Chromium\Application\chrome.exe",
    ];
    for rel in rels {
        for root in roots {
            if let Some(r) = env::var_os(root) {
                v.push(PathBuf::from(r).join(rel));
            }
        }
    }
    v
}

#[cfg(target_os = "macos")]
fn candidates() -> Vec<PathBuf> {
    let apps = [
        "Google Chrome.app/Contents/MacOS/Google Chrome",
        "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "Brave Browser.app/Contents/MacOS/Brave Browser",
        "Chromium.app/Contents/MacOS/Chromium",
    ];
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(h) = env::var_os("HOME") {
        roots.push(PathBuf::from(h).join("Applications"));
    }
    roots.iter().flat_map(|r| apps.iter().map(move |a| r.join(a))).collect()
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn candidates() -> Vec<PathBuf> {
    let names = [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "microsoft-edge",
        "microsoft-edge-stable",
        "brave-browser",
    ];
    let dirs: Vec<PathBuf> = env::var_os("PATH").map(|p| env::split_paths(&p).collect()).unwrap_or_default();
    let mut v: Vec<PathBuf> = names.iter().flat_map(|n| dirs.iter().map(move |d| d.join(n))).collect();
    // Snap and Flatpak browsers are not on PATH for every desktop.
    v.push(PathBuf::from("/snap/bin/chromium"));
    v
}

/// The arguments that make `browser` show `url` as an app window.
pub fn app_args(url: &str, profile: &Path) -> Vec<String> {
    vec![
        format!("--app={url}"),
        format!("--user-data-dir={}", profile.display()),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--window-size=1440,900".into(),
    ]
}

/// Open `url` in an app window, or in the default browser when no
/// Chromium-based browser is installed. Returns what was used.
pub fn open_window(url: &str, profile: &Path) -> Result<String, String> {
    if let Some(b) = find_browser() {
        let _ = std::fs::create_dir_all(profile);
        Command::new(&b)
            .args(app_args(url, profile))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{}: {e}", b.display()))?;
        return Ok(b.display().to_string());
    }
    open_default(url)?;
    Ok("the default browser".into())
}

#[cfg(target_os = "windows")]
fn open_default(url: &str) -> Result<(), String> {
    Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn open_default(url: &str) -> Result<(), String> {
    Command::new("open").arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn open_default(url: &str) -> Result<(), String> {
    Command::new("xdg-open").arg(url).spawn().map(|_| ()).map_err(|e| format!("xdg-open: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_make_an_app_window_with_own_profile() {
        let a = app_args("http://127.0.0.1:1/", Path::new("/p"));
        assert_eq!(a[0], "--app=http://127.0.0.1:1/");
        assert!(a.iter().any(|x| x == "--user-data-dir=/p"));
    }
}
