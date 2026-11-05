//! Files, the way a program on this system handles them: the system's open
//! and save dialogs, reading and writing in place (Android content links
//! included, through the fs plugin), a plotted PDF handed to the system's
//! viewer, and the drawings the app was opened with.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_dialog::{DialogExt, FilePath};
use tauri_plugin_fs::{FsExt, OpenOptions};

/// A file the workbench will ask to read: its name and where it is (a path,
/// or on Android a content link).
#[derive(Clone, Debug, Serialize)]
pub struct Picked {
    pub name: String,
    pub path: String,
}

/// The drawings the app was opened with, until the workbench asks for them.
#[derive(Default)]
pub struct Launch(pub Mutex<Vec<Picked>>);

const DRAWINGS: &[&str] = &["dwg", "dxf", "cad"];

fn picked(p: &FilePath) -> Picked {
    let path = p.to_string();
    let name = match p {
        FilePath::Path(pb) => pb.file_name().map(|n| n.to_string_lossy().into_owned()),
        FilePath::Url(u) => u.path_segments().and_then(|mut s| s.next_back()).map(|s| percent_decode(s)),
    }
    .unwrap_or_else(|| "drawing".into());
    Picked { name, path }
}

/// The drawings in a list of program arguments (what a file manager passes
/// when a drawing is opened with BlackCAD).
pub fn drawings_in(args: impl IntoIterator<Item = String>) -> Vec<Picked> {
    args.into_iter()
        .filter(|a| !a.starts_with('-'))
        .map(PathBuf::from)
        .filter(|p| {
            p.extension()
                .map(|e| DRAWINGS.contains(&e.to_string_lossy().to_ascii_lowercase().as_str()))
                .unwrap_or(false)
                && p.is_file()
        })
        .map(|p| picked(&FilePath::Path(p)))
        .collect()
}

/// Drawings opened with the app while it runs: the workbench opens them.
pub fn open_later<R: Runtime>(app: &AppHandle<R>, files: Vec<Picked>) {
    if files.is_empty() {
        return;
    }
    let _ = app.emit("open-files", files);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[tauri::command(async)]
pub fn open_dialog<R: Runtime>(app: AppHandle<R>) -> Vec<Picked> {
    app.dialog()
        .file()
        .set_title("Open drawings")
        .add_filter("Drawings", DRAWINGS)
        .blocking_pick_files()
        .unwrap_or_default()
        .iter()
        .map(picked)
        .collect()
}

#[tauri::command(async)]
pub fn save_dialog<R: Runtime>(app: AppHandle<R>, name: String, ext: String, description: String) -> Option<String> {
    app.dialog()
        .file()
        .set_title("Save")
        .set_file_name(name)
        .add_filter(description, &[ext.as_str()])
        .blocking_save_file()
        .map(|p| p.to_string())
}

fn file_path(s: &str) -> Result<FilePath, String> {
    s.parse::<FilePath>().map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn read_file<R: Runtime>(app: AppHandle<R>, path: String) -> Result<Response, String> {
    let mut f = app
        .fs()
        .open(file_path(&path)?, OpenOptions::new().read(true).clone())
        .map_err(|e| format!("{path}: {e}"))?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).map_err(|e| format!("{path}: {e}"))?;
    Ok(Response::new(bytes))
}

/// Writes the raw body to the path in the `x-path` header, replacing the file.
#[tauri::command(async)]
pub fn write_file<R: Runtime>(app: AppHandle<R>, request: Request<'_>) -> Result<(), String> {
    let path = header(&request, "x-path")?;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("write takes the file's bytes".into());
    };
    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    let mut f = app.fs().open(file_path(&path)?, opts).map_err(|e| format!("{path}: {e}"))?;
    f.write_all(bytes).and_then(|_| f.flush()).map_err(|e| format!("{path}: {e}"))
}

/// A plotted sheet, put in the temporary folder and opened in the system's
/// PDF viewer, whose print dialog chooses the printer.
#[tauri::command(async)]
pub fn show_pdf<R: Runtime>(app: AppHandle<R>, request: Request<'_>) -> Result<(), String> {
    let name = header(&request, "x-name")?;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("a PDF's bytes are needed".into());
    };
    let safe: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let dir = app.path().temp_dir().map_err(|e| e.to_string())?.join("BlackCAD");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(safe);
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    open_with_system(&app, &path)
}

#[cfg(desktop)]
fn open_with_system<R: Runtime>(app: &AppHandle<R>, path: &std::path::Path) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

#[cfg(mobile)]
fn open_with_system<R: Runtime>(app: &AppHandle<R>, path: &std::path::Path) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url(format!("file://{}", path.display()), None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn launch_files(launch: State<'_, Launch>) -> Vec<Picked> {
    std::mem::take(&mut *launch.0.lock().unwrap_or_else(|p| p.into_inner()))
}

#[tauri::command]
pub fn set_title<R: Runtime>(app: AppHandle<R>, title: String) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_title(&title);
    }
}

fn header(request: &Request<'_>, name: &str) -> Result<String, String> {
    let v = request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| format!("missing {name}"))?;
    Ok(percent_decode(v))
}

/// %XX escapes (encodeURIComponent) back to text.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::percent_decode;

    #[test]
    fn decodes_what_encode_uri_component_makes() {
        assert_eq!(percent_decode("C%3A%5CUsers%5Cme%5CPart%20A%20%C3%98.dwg"), "C:\\Users\\me\\Part A Ø.dwg");
        assert_eq!(percent_decode("plain.cad"), "plain.cad");
        assert_eq!(percent_decode("end%2"), "end%2");
        assert_eq!(percent_decode("%41"), "A");
    }
}
