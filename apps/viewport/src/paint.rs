//! The egui side of the viewport: lay out the canvas and the status bar,
//! feed the pointer, wheel and keys to the [`Viewer`], and paint the shapes
//! it computes.

use cad_view::{Prim, Rgb, Shape, Tool, Viewer};
use egui::{Align2, Color32, FontId, Mesh, Pos2, Rect, Sense, Stroke};

const CANVAS: Color32 = Color32::from_rgb(0x14, 0x15, 0x17);
const PAPER: Color32 = Color32::WHITE;
const CHROME: Color32 = Color32::from_rgb(0x1e, 0x20, 0x24);
const GRID_MINOR: Color32 = Color32::from_rgb(0x24, 0x26, 0x2a);
const GRID_MAJOR: Color32 = Color32::from_rgb(0x34, 0x37, 0x3d);
const AXIS_X: Color32 = Color32::from_rgb(0xa8, 0x3c, 0x3c);
const AXIS_Y: Color32 = Color32::from_rgb(0x3c, 0xa8, 0x5a);
const HOVER: Color32 = Color32::from_rgb(0x4f, 0xc3, 0xf7);
const SNAP: Color32 = Color32::from_rgb(0xff, 0xd5, 0x4f);
const SELECT: Color32 = Color32::from_rgb(0xff, 0x9f, 0x1c);
const PREVIEW: Color32 = Color32::from_rgb(0xe0, 0xe0, 0xe0);
const CROSSHAIR: Color32 = Color32::from_rgb(0x70, 0x74, 0x7c);
const TEXT: Color32 = Color32::from_rgb(0xc8, 0xcc, 0xd2);

/// Points of a wheel line, and of a page, for [`wheel_points`].
const LINE_POINTS: f32 = 40.0;
const PAGE_POINTS: f32 = 500.0;

/// What the interface asks the app to do, which needs the operating system
/// (a file dialog, closing the window).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    New { imperial: bool },
    Open,
    Sample,
    SaveAs(&'static str),
    Quit,
}

/// What the interface keeps between frames.
pub struct State {
    /// The command line's text.
    pub cmd: String,
    pub show_side: bool,
}

impl Default for State {
    fn default() -> Self {
        State { cmd: String::new(), show_side: true }
    }
}

const SNAP_KINDS: [(u32, &str); 8] = [
    (1, "Endpoint"),
    (2, "Midpoint"),
    (4, "Center"),
    (8, "Quadrant"),
    (16, "Intersection"),
    (32, "Nearest"),
    (64, "Node"),
    (128, "Insertion"),
];

fn cmd_id() -> egui::Id {
    egui::Id::unique("blackcad_command_line")
}

pub fn show(viewer: &mut Viewer, st: &mut State, ui: &mut egui::Ui) -> Vec<Action> {
    let mut actions = Vec::new();
    let ctx = ui.ctx().clone();

    // Typing anywhere goes to the command line, as in the CAD programs.
    let typing = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(_))));
    if typing && ctx.memory(|m| m.focused().is_none()) {
        ctx.memory_mut(|m| m.request_focus(cmd_id()));
    }
    let no_focus = ctx.memory(|m| m.focused().is_none());
    let (undo, redo, esc, delete, home, f7, f3, open, save) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z),
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y),
            i.key_pressed(egui::Key::Escape),
            no_focus && i.key_pressed(egui::Key::Delete),
            no_focus && i.key_pressed(egui::Key::Home),
            i.key_pressed(egui::Key::F7),
            i.key_pressed(egui::Key::F3),
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::O),
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::S),
        )
    });
    if undo {
        viewer.undo();
    }
    if redo {
        viewer.redo();
    }
    if esc {
        viewer.cancel();
        st.cmd.clear();
    }
    if delete {
        viewer.erase_selected();
    }
    if home {
        viewer.zoom_extents();
    }
    if f7 {
        viewer.show_grid = !viewer.show_grid;
    }
    if f3 {
        viewer.snap_on = !viewer.snap_on;
    }
    if open {
        actions.push(Action::Open);
    }
    if save {
        actions.push(Action::SaveAs("dxf"));
    }

    // From the bottom up: status bar, command line.
    egui::Panel::bottom("status").exact_size(24.0).frame(egui::Frame::NONE.fill(CHROME)).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(viewer.status()).monospace().color(TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                let on = |b: bool| if b { "on" } else { "off" };
                ui.label(
                    egui::RichText::new(format!(
                        "{}   GRID {} (F7)   SNAP {} (F3)   {}",
                        viewer.tool.name(),
                        on(viewer.show_grid),
                        on(viewer.snap_on),
                        viewer.units
                    ))
                    .color(TEXT),
                );
            });
        });
    });
    egui::Panel::bottom("command").frame(egui::Frame::NONE.fill(CHROME).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
        let from = viewer.log.len().saturating_sub(5);
        egui::ScrollArea::vertical().max_height(86.0).stick_to_bottom(true).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            for l in &viewer.log[from..] {
                ui.label(egui::RichText::new(l).monospace().size(12.0).color(TEXT));
            }
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Command:").monospace().color(SNAP));
            let prompt = viewer.prompt();
            let edit = ui.add(
                egui::TextEdit::singleline(&mut st.cmd)
                    .id(cmd_id())
                    .font(egui::TextStyle::Monospace)
                    .hint_text(prompt)
                    .desired_width(f32::INFINITY),
            );
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let line = std::mem::take(&mut st.cmd);
                viewer.run(&line);
                edit.request_focus();
            }
        });
    });
    menu_bar(viewer, st, ui, &mut actions);
    tool_bar(viewer, ui, &mut actions);
    if st.show_side {
        egui::Panel::right("side").exact_size(250.0).frame(egui::Frame::NONE.fill(CHROME).inner_margin(egui::Margin::same(8))).show(ui, |ui| {
            side_panel(viewer, ui);
        });
    }
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(if viewer.options.paper { PAPER } else { CANVAS })).show(ui, |ui| {
        canvas(viewer, ui);
    });
    actions
}

fn menu_bar(viewer: &mut Viewer, st: &mut State, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    egui::Panel::top("menu").exact_size(26.0).frame(egui::Frame::NONE.fill(CHROME).inner_margin(egui::Margin::symmetric(6, 0))).show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New drawing (millimetres)").clicked() {
                    actions.push(Action::New { imperial: false });
                    ui.close();
                }
                if ui.button("New drawing (inches)").clicked() {
                    actions.push(Action::New { imperial: true });
                    ui.close();
                }
                if ui.add(egui::Button::new("Open…").shortcut_text("Ctrl+O")).clicked() {
                    actions.push(Action::Open);
                    ui.close();
                }
                if ui.button("Open the sample plate").clicked() {
                    actions.push(Action::Sample);
                    ui.close();
                }
                ui.separator();
                for (label, fmt) in [
                    ("Save as DXF…", "dxf"),
                    ("Save as DWG…", "dwg"),
                    ("Save as BlackCAD (.cad)…", "cad"),
                    ("Export SVG…", "svg"),
                    ("Export PDF…", "pdf"),
                ] {
                    if ui.button(label).clicked() {
                        actions.push(Action::SaveAs(fmt));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    actions.push(Action::Quit);
                    ui.close();
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.add_enabled(viewer.undo_depth > 0, egui::Button::new("Undo").shortcut_text("Ctrl+Z")).clicked() {
                    viewer.undo();
                    ui.close();
                }
                if ui.add_enabled(viewer.redo_depth > 0, egui::Button::new("Redo").shortcut_text("Ctrl+Y")).clicked() {
                    viewer.redo();
                    ui.close();
                }
                ui.separator();
                if ui.add_enabled(!viewer.selected.is_empty(), egui::Button::new("Delete").shortcut_text("Del")).clicked() {
                    viewer.erase_selected();
                    ui.close();
                }
                if ui.add_enabled(!viewer.selected.is_empty(), egui::Button::new("Select none").shortcut_text("Esc")).clicked() {
                    viewer.select(&[]);
                    ui.close();
                }
            });
            ui.menu_button("Draw", |ui| {
                for t in Tool::ALL {
                    if ui.selectable_label(viewer.tool == t, t.name()).clicked() {
                        viewer.set_tool(t);
                        ui.close();
                    }
                }
            });
            ui.menu_button("View", |ui| {
                if ui.add(egui::Button::new("Zoom to the drawing").shortcut_text("Home")).clicked() {
                    viewer.zoom_extents();
                    ui.close();
                }
                ui.checkbox(&mut viewer.show_grid, "Grid (F7)");
                ui.checkbox(&mut viewer.snap_on, "Object snap (F3)");
                ui.checkbox(&mut viewer.options.paper, "Paper colours");
                ui.checkbox(&mut st.show_side, "Layers and properties");
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Commands").clicked() {
                    viewer.run("help");
                    ui.close();
                }
            });
        });
    });
}

fn tool_bar(viewer: &mut Viewer, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    egui::Panel::top("tools").exact_size(36.0).frame(egui::Frame::NONE.fill(CHROME).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            if ui.button("Open").clicked() {
                actions.push(Action::Open);
            }
            if ui.button("Save").clicked() {
                actions.push(Action::SaveAs("dxf"));
            }
            ui.separator();
            if ui.add_enabled(viewer.undo_depth > 0, egui::Button::new("Undo")).clicked() {
                viewer.undo();
            }
            if ui.add_enabled(viewer.redo_depth > 0, egui::Button::new("Redo")).clicked() {
                viewer.redo();
            }
            ui.separator();
            for t in Tool::ALL {
                if ui.selectable_label(viewer.tool == t, t.name()).clicked() {
                    viewer.set_tool(t);
                }
            }
            ui.separator();
            if ui.button("Zoom to drawing").clicked() {
                viewer.zoom_extents();
            }
            ui.toggle_value(&mut viewer.show_grid, "Grid");
            ui.toggle_value(&mut viewer.snap_on, "Snap");
            ui.toggle_value(&mut viewer.options.paper, "Paper");
        });
    });
}

fn side_panel(viewer: &mut Viewer, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Layers").strong().color(TEXT));
    let layers = viewer.layer_infos.clone();
    let mut toggle = None;
    let mut current = None;
    egui::ScrollArea::vertical().id_salt("layers").max_height(240.0).show(ui, |ui| {
        for l in &layers {
            ui.horizontal(|ui| {
                let mut on = l.on;
                if ui.checkbox(&mut on, "").changed() {
                    toggle = Some((l.handle, on));
                }
                ui.label(egui::RichText::new("■").color(color(l.color)));
                let name = if l.count > 0 { format!("{} ({})", l.name, l.count) } else { l.name.clone() };
                if ui.selectable_label(l.current, name).clicked() {
                    current = Some(l.handle);
                }
            });
        }
    });
    if let Some((h, on)) = toggle {
        viewer.set_layer_on(h, on);
    }
    if let Some(h) = current {
        viewer.set_layer_current(h);
    }
    ui.add_space(8.0);
    ui.collapsing("Object snap", |ui| {
        for (bit, name) in SNAP_KINDS {
            let mut on = viewer.snap_kinds & bit != 0;
            if ui.checkbox(&mut on, name).changed() {
                viewer.snap_kinds = if on { viewer.snap_kinds | bit } else { viewer.snap_kinds & !bit };
            }
        }
    });
    ui.add_space(8.0);
    ui.label(egui::RichText::new("Selection").strong().color(TEXT));
    egui::ScrollArea::vertical().id_salt("selection").show(ui, |ui| {
        if viewer.selection_text.is_empty() {
            ui.label(egui::RichText::new("Nothing selected").color(GRID_MAJOR));
        } else {
            ui.label(egui::RichText::new(&viewer.selection_text).monospace().size(12.0).color(TEXT));
        }
    });
}

fn canvas(viewer: &mut Viewer, ui: &mut egui::Ui) {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    viewer.set_size([rect.width(), rect.height()]);
    let origin = rect.min;
    let local = |p: Pos2| [p.x - origin.x, p.y - origin.y];

    // Drag pans; a click is a click. The wheel zooms about the pointer.
    if response.dragged() {
        let d = response.drag_delta();
        viewer.pan(d.x, d.y);
    }
    if response.hovered() {
        let points = ui.input(|i| wheel_points(&i.raw.events));
        if let Some(p) = response.hover_pos() {
            viewer.wheel(f64::from(points), local(p));
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    let hovered = response.hover_pos().filter(|_| !response.dragged());
    viewer.hover_at(hovered.map(local));
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        let add = ui.input(|i| i.modifiers.shift);
        viewer.canvas_click(local(p), add);
    }
    if response.secondary_clicked() {
        viewer.finish(false);
    }

    let painter = ui.painter_at(rect);
    let at = |p: [f32; 2]| Pos2::new(origin.x + p[0], origin.y + p[1]);
    paint_grid(viewer, &painter, rect, &at);
    for p in viewer.prims() {
        paint_prim(&painter, &p, &at, None);
    }
    for p in viewer.selection_prims() {
        paint_prim(&painter, &p, &at, Some(SELECT));
    }
    for p in viewer.hover_prims() {
        paint_prim(&painter, &p, &at, Some(HOVER));
    }
    for p in viewer.preview_prims() {
        paint_prim(&painter, &p, &at, Some(PREVIEW));
    }
    for q in &viewer.points {
        painter.circle_stroke(at(viewer.view.to_screen(*q)), 3.5, Stroke::new(1.5, SNAP));
    }
    if let Some(h) = &viewer.hover {
        let c = at(viewer.view.to_screen(h.snap.as_ref().map_or(h.world, |s| s.point)));
        let arm = 12.0;
        painter.line_segment([c - egui::vec2(arm, 0.0), c + egui::vec2(arm, 0.0)], Stroke::new(1.0, CROSSHAIR));
        painter.line_segment([c - egui::vec2(0.0, arm), c + egui::vec2(0.0, arm)], Stroke::new(1.0, CROSSHAIR));
        if let Some(s) = &h.snap {
            painter.rect_stroke(Rect::from_center_size(c, egui::Vec2::splat(10.0)), 0.0, Stroke::new(1.5, SNAP), egui::StrokeKind::Middle);
            painter.text(c + egui::vec2(10.0, -10.0), Align2::LEFT_BOTTOM, &s.name, FontId::proportional(12.0), SNAP);
        }
    }
}

/// The vertical wheel scroll of this frame in points, upward positive. A
/// pinch or ctrl-scroll is egui's own zoom and is not counted here.
fn wheel_points(events: &[egui::Event]) -> f32 {
    let total: f32 = events
        .iter()
        .filter_map(|e| match e {
            egui::Event::MouseWheel { unit, delta, modifiers, .. } if !modifiers.command && !modifiers.ctrl => Some(
                delta.y
                    * match unit {
                        egui::MouseWheelUnit::Point => 1.0,
                        egui::MouseWheelUnit::Line => LINE_POINTS,
                        egui::MouseWheelUnit::Page => PAGE_POINTS,
                    },
            ),
            _ => None,
        })
        .sum();
    if total.is_finite() { total } else { 0.0 }
}

fn color(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

fn paint_grid(viewer: &Viewer, painter: &egui::Painter, rect: Rect, at: &impl Fn([f32; 2]) -> Pos2) {
    let paper = viewer.options.paper;
    let Some(grid) = viewer.grid() else { return };
    let (minor, major) = if paper { (Color32::from_gray(0xee), Color32::from_gray(0xd8)) } else { (GRID_MINOR, GRID_MAJOR) };
    let shapes: Vec<egui::Shape> = grid
        .lines
        .iter()
        .map(|l| egui::Shape::line_segment([at(l.a), at(l.b)], Stroke::new(1.0, if l.major { major } else { minor })))
        .collect();
    painter.extend(shapes);
    if let Some(y) = grid.x_axis {
        painter.line_segment([Pos2::new(rect.left(), rect.top() + y), Pos2::new(rect.right(), rect.top() + y)], Stroke::new(1.0, AXIS_X));
    }
    if let Some(x) = grid.y_axis {
        painter.line_segment([Pos2::new(rect.left() + x, rect.top()), Pos2::new(rect.left() + x, rect.bottom())], Stroke::new(1.0, AXIS_Y));
    }
}

/// Paint one shape; `over` paints it in that colour instead of its own.
fn paint_prim(painter: &egui::Painter, prim: &Prim, at: &impl Fn([f32; 2]) -> Pos2, over: Option<Color32>) {
    let col = over.unwrap_or_else(|| color(prim.color));
    let width = if over.is_some() { prim.width + 1.5 } else { prim.width };
    match &prim.shape {
        Shape::Line { points, closed } => {
            let pts: Vec<Pos2> = points.iter().map(|p| at(*p)).collect();
            if pts.len() == 2 {
                painter.line_segment([pts[0], pts[1]], Stroke::new(width, col));
            } else if *closed {
                painter.add(egui::Shape::closed_line(pts, Stroke::new(width, col)));
            } else {
                painter.add(egui::Shape::line(pts, Stroke::new(width, col)));
            }
        }
        Shape::Fill { triangles } => {
            let mut mesh = Mesh::default();
            for tri in triangles.as_chunks::<3>().0 {
                let i = mesh.vertices.len() as u32;
                for p in tri {
                    mesh.colored_vertex(at(*p), col);
                }
                mesh.add_triangle(i, i + 1, i + 2);
            }
            if !mesh.vertices.is_empty() {
                painter.add(egui::Shape::mesh(mesh));
            }
        }
        Shape::Dot(p) => {
            painter.circle_filled(at(*p), (width * 0.5).max(1.0) + 0.5, col);
        }
    }
}
