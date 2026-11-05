//! BlackCAD as an application for Windows, macOS, Linux and Android.
//!
//! One window shows the workbench (apps/web, built into dist/web) in the
//! system's own webview: WebView2 on Windows, WKWebView on macOS, WebKitGTK
//! on Linux, the Android System WebView on Android. The Rust engine runs in
//! this process, natively (engine.rs); the page reaches it, and the system's
//! file dialogs (files.rs), through the bridge in bridge.js.

mod engine;
mod files;

#[cfg(desktop)]
use tauri::Manager;
use tauri::{WebviewUrl, WebviewWindowBuilder};

/// The name host.js knows this system by.
fn platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "ios") {
        "ios"
    } else {
        "linux"
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();

    // A drawing opened with BlackCAD while it runs goes to the window that
    // is open, not a second copy of the program.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
        files::open_later(app, files::drawings_in(args.into_iter().skip(1)));
    }));

    let app = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .manage(engine::Engine::new())
        .manage(files::Launch::default())
        .invoke_handler(tauri::generate_handler![
            engine::engine_version,
            engine::engine_restart,
            engine::engine_open,
            engine::engine_new,
            engine::engine_sample,
            engine::engine_close,
            engine::engine_text,
            engine::engine_display,
            engine::engine_save,
            engine::engine_save_notes,
            files::open_dialog,
            files::save_dialog,
            files::read_file,
            files::write_file,
            files::show_pdf,
            files::launch_files,
            files::set_title,
        ])
        .setup(|app| {
            #[cfg(desktop)]
            {
                let launched = files::drawings_in(std::env::args().skip(1));
                *app.state::<files::Launch>().0.lock().unwrap() = launched;
            }
            let bridge = include_str!("bridge.js").replace("__PLATFORM__", platform());
            let w = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .initialization_script(&bridge)
                // Files dropped on the window reach the page as they do in a
                // browser; the workbench opens them.
                .disable_drag_drop_handler();
            #[cfg(desktop)]
            let w = w.title("BlackCAD").inner_size(1360.0, 860.0).min_inner_size(720.0, 480.0).center();
            w.build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("BlackCAD could not start");

    app.run(|_app, _event| {
        // macOS hands files opened from Finder to the running app as an
        // event, not as arguments.
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Opened { urls } = _event {
            let paths = urls.into_iter().filter_map(|u| u.to_file_path().ok()).map(|p| p.to_string_lossy().into_owned());
            let files = files::drawings_in(paths);
            let launch = _app.state::<files::Launch>();
            if _app.get_webview_window("main").is_some() {
                files::open_later(_app, files);
            } else {
                launch.0.lock().unwrap().extend(files);
            }
        }
    });
}
