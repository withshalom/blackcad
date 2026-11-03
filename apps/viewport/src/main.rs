//! BlackCAD's desktop viewport.
//!
//! Usage: `blackcad [--sample] [drawing.dwg|drawing.dxf|drawing.cad]`
//!
//! A window that opens a drawing (from the command line, or dropped on the
//! window), pans with the middle or left button, zooms with the wheel about
//! the pointer, fits with `F` or `Home`, switches the grid with `G`, paper
//! colours with `P`, and snaps and picks under the pointer. What to draw and
//! where is `cad-view`'s; this file only paints it and reads the mouse
//! (the egui layer of CADCraft's canvas, MIT OR Apache-2.0; see NOTICE).
#![forbid(unsafe_code)]
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod paint;

use cad_view::Viewer;

struct App {
    viewer: Viewer,
}

impl App {
    fn open_path(&mut self, path: &std::path::Path) {
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        self.viewer.message = match std::fs::read(path) {
            Ok(bytes) => match self.viewer.open(&name, &bytes) {
                Ok(()) => return,
                Err(e) => format!("{name}: {e}"),
            },
            Err(e) => format!("{name}: {e}"),
        };
    }

    fn open_dropped(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
        for file in dropped {
            let path = file.path();
            if !path.as_os_str().is_empty() {
                self.open_path(path);
            }
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.open_dropped(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        paint::show(&mut self.viewer, ui);
    }
}

fn main() -> eframe::Result<()> {
    let mut first = None;
    let mut sample = false;
    for a in std::env::args_os().skip(1) {
        if a == "--sample" {
            sample = true;
        } else if a == "--help" || a == "-h" {
            println!("Usage: blackcad [--sample] [drawing.dwg|drawing.dxf|drawing.cad]");
            return Ok(());
        } else if first.is_none() {
            first = Some(std::path::PathBuf::from(a));
        }
    }
    let mut app = App { viewer: Viewer::new() };
    if sample && let Err(e) = app.viewer.open_sample() {
        app.viewer.message = e;
    }
    if let Some(path) = first {
        app.open_path(&path);
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("BlackCAD")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([480.0, 320.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("BlackCAD", options, Box::new(move |_cc| Ok(Box::new(app))))
}
