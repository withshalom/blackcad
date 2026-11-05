//! The BlackCAD engine in the app's own process: `cad_ffi::session::Session`,
//! the same code core.wasm runs on the web, compiled natively, with the
//! computer's memory and threads. The workbench calls it through the
//! commands below (apps/shell/src/bridge.js, apps/web/core.js NativeBackend).
//!
//! One session holds every open drawing. Calls take it one at a time. A
//! panic in the engine is caught: that call fails as a crash, the session is
//! dropped, and the workbench restarts it and reopens its drawings, as it
//! does after a crash in core.wasm.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;

use cad_ffi::session::Session;
use serde::Serialize;
use tauri::State;
use tauri::ipc::{InvokeBody, Request, Response};

/// What failed, as the workbench rebuilds it (core.js `rebuildError`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineError {
    message: String,
    crash: bool,
    out_of_memory: bool,
}

impl EngineError {
    fn plain(message: impl Into<String>) -> Self {
        Self { message: message.into(), crash: false, out_of_memory: false }
    }
    fn crash(message: impl Into<String>) -> Self {
        Self { message: message.into(), crash: true, out_of_memory: false }
    }
}

type Result<T> = std::result::Result<T, EngineError>;

#[derive(Default)]
pub struct Engine {
    /// None after a crash, until the workbench restarts it.
    session: Mutex<Option<Session>>,
    /// The notes from the last save (what the format could not hold).
    notes: Mutex<Vec<String>>,
}

impl Engine {
    pub fn new() -> Self {
        Self { session: Mutex::new(Some(Session::new())), notes: Mutex::new(Vec::new()) }
    }

    /// Runs `f` on the session, turning a panic into a crash.
    fn with<T>(&self, f: impl FnOnce(&mut Session) -> std::result::Result<T, String>) -> Result<T> {
        let mut guard = self.session.lock().unwrap_or_else(|p| p.into_inner());
        let Some(session) = guard.as_mut() else {
            return Err(EngineError::crash("the engine stopped earlier and has not been restarted"));
        };
        match catch_unwind(AssertUnwindSafe(|| f(session))) {
            Ok(r) => r.map_err(EngineError::plain),
            Err(payload) => {
                *guard = None;
                let msg = payload
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "the engine stopped on a bug".into());
                Err(EngineError::crash(msg))
            }
        }
    }
}

/// "BlackCAD 0.17.0", as core.wasm names itself.
#[tauri::command]
pub fn engine_version() -> String {
    format!("{} {}", cad_core::PRODUCT_NAME, cad_core::VERSION)
}

#[tauri::command]
pub fn engine_restart(engine: State<'_, Engine>) {
    *engine.session.lock().unwrap_or_else(|p| p.into_inner()) = Some(Session::new());
}

/// A drawing from a file's bytes, sent as the raw body of the call.
#[tauri::command(async)]
pub fn engine_open(engine: State<'_, Engine>, request: Request<'_>) -> Result<u32> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err(EngineError::plain("open takes the file's bytes"));
    };
    engine.with(|s| s.open(bytes))
}

#[tauri::command(async)]
pub fn engine_new(engine: State<'_, Engine>, imperial: bool) -> Result<u32> {
    engine.with(|s| s.new_drawing(imperial))
}

#[tauri::command(async)]
pub fn engine_sample(engine: State<'_, Engine>) -> Result<u32> {
    engine.with(|s| s.sample())
}

#[tauri::command(async)]
pub fn engine_close(engine: State<'_, Engine>, id: u32) -> Result<()> {
    engine.with(|s| {
        s.close(id);
        Ok(())
    })
}

/// `info` and `exec` answer JSON text; the workbench parses it.
#[tauri::command(async)]
pub fn engine_text(engine: State<'_, Engine>, method: String, id: u32, line: Option<String>) -> Result<String> {
    engine.with(|s| match method.as_str() {
        "info" => s.info(id),
        "exec" => s.exec(id, line.as_deref().unwrap_or("")),
        _ => Err(format!("the engine has no call {method:?}")),
    })
}

/// The display list (or what changed in it) as raw bytes.
#[tauri::command(async)]
pub fn engine_display(engine: State<'_, Engine>, id: u32, changes: bool) -> Result<Response> {
    let bytes = engine.with(|s| if changes { s.display_changes(id) } else { s.display(id) })?;
    Ok(Response::new(bytes))
}

/// The drawing in a file format, as raw bytes; its notes wait for
/// `engine_save_notes`, which the bridge asks for next.
#[tauri::command(async)]
pub fn engine_save(engine: State<'_, Engine>, id: u32, format: String, user: String) -> Result<Response> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (bytes, notes) = engine.with(|s| s.save(id, &format, now, &user))?;
    *engine.notes.lock().unwrap_or_else(|p| p.into_inner()) = notes;
    Ok(Response::new(bytes))
}

#[tauri::command]
pub fn engine_save_notes(engine: State<'_, Engine>) -> Vec<String> {
    std::mem::take(&mut *engine.notes.lock().unwrap_or_else(|p| p.into_inner()))
}
