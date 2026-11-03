//! BlackCAD's desktop app.
//!
//! Usage: `blackcad [--sample] [drawing.dwg|drawing.dxf|drawing.cad]`
//!
//! A window with a menu bar, a toolbar, a command line and a layers panel
//! over a drawing canvas. It opens DWG, DXF and BlackCAD (`.cad`) files (File
//! menu, Ctrl+O, or dropped on the window), draws lines, circles, rectangles,
//! polylines and arcs by clicking or typing, selects and erases, undoes and
//! redoes, and saves as DXF, DWG or `.cad`. The wheel zooms, dragging pans,
//! and object snaps follow the pointer. What to draw and where, and what
//! every command does, is `cad-view`'s and the core's; this crate only
//! paints and reads the mouse and keyboard (the egui layer of CADCraft's
//! canvas, MIT OR Apache-2.0; see NOTICE).
#![forbid(unsafe_code)]
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod paint;

use cad_view::Viewer;
use paint::{Action, State};

struct App {
    viewer: Viewer,
    ui: State,
    /// The window title last sent, to send it only when it changes.
    title: String,
}

impl App {
    fn open_path(&mut self, path: &std::path::Path) {
        if !self.confirm_discard() {
            return;
        }
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        match std::fs::read(path) {
            Ok(bytes) => match self.viewer.open(&name, &bytes) {
                Ok(()) => self.viewer.log_line(format!("opened {name}")),
                Err(e) => self.viewer.log_line(format!("{name}: {e}")),
            },
            Err(e) => self.viewer.log_line(format!("{name}: {e}")),
        }
    }

    /// Whether to go on when the open drawing has unsaved edits.
    fn confirm_discard(&self) -> bool {
        !self.viewer.dirty
            || rfd::MessageDialog::new()
                .set_title("BlackCAD")
                .set_description("This drawing has changes that are not saved. Discard them?")
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                == rfd::MessageDialogResult::Yes
    }

    fn save_as(&mut self, format: &'static str) {
        let stem = std::path::Path::new(&self.viewer.name).file_stem().map_or_else(|| "drawing".to_string(), |s| s.to_string_lossy().into_owned());
        let ext = if format == "dxfb" { "dxf" } else { format };
        let Some(path) = rfd::FileDialog::new().set_file_name(format!("{stem}.{ext}")).add_filter(format.to_uppercase(), &[ext]).save_file() else {
            return;
        };
        match self.viewer.save_bytes(format) {
            Ok((bytes, notes)) => match std::fs::write(&path, bytes) {
                Ok(()) => {
                    self.viewer.log_line(format!("saved {}", path.display()));
                    for n in notes {
                        self.viewer.log_line(format!("note: {n}"));
                    }
                }
                Err(e) => self.viewer.log_line(format!("{}: {e}", path.display())),
            },
            Err(e) => self.viewer.log_line(e),
        }
    }

    fn act(&mut self, ctx: &egui::Context, a: Action) {
        match a {
            Action::New { imperial } => {
                if self.confirm_discard()
                    && let Err(e) = self.viewer.new_drawing(imperial)
                {
                    self.viewer.log_line(e);
                }
            }
            Action::Open => {
                if let Some(path) = rfd::FileDialog::new().add_filter("Drawings (DWG, DXF, .cad)", &["dwg", "dxf", "cad"]).pick_file() {
                    self.open_path(&path);
                }
            }
            Action::Sample => {
                if self.confirm_discard()
                    && let Err(e) = self.viewer.open_sample()
                {
                    self.viewer.log_line(e);
                }
            }
            Action::SaveAs(format) => self.save_as(format),
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
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
        let actions = paint::show(&mut self.viewer, &mut self.ui, ui);
        let ctx = ui.ctx().clone();
        for a in actions {
            self.act(&ctx, a);
        }
        let title = format!("{} - BlackCAD", self.viewer.title());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

fn main() -> eframe::Result<()> {
    let mut app = App { viewer: Viewer::new(), ui: State::default(), title: String::new() };
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
    let started = if sample { app.viewer.open_sample() } else { app.viewer.new_drawing(false) };
    if let Err(e) = started {
        app.viewer.log_line(e);
    }
    if let Some(path) = first {
        app.open_path(&path);
    }
    app.viewer.log_line("Type help for the commands; click a tool or type line, circle, rect, pline, arc.");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("BlackCAD")
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([720.0, 480.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("BlackCAD", options, Box::new(move |_cc| Ok(Box::new(app))))
}
