//! The egui side of the viewport: lay out the canvas and the status bar,
//! feed the pointer, wheel and keys to the [`Viewer`], and paint the shapes
//! it computes.

use cad_view::{Prim, Rgb, Shape, Viewer};
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
const TEXT: Color32 = Color32::from_rgb(0xc8, 0xcc, 0xd2);

/// Points of a wheel line, and of a page, for [`wheel_points`].
const LINE_POINTS: f32 = 40.0;
const PAGE_POINTS: f32 = 500.0;

pub fn show(viewer: &mut Viewer, ui: &mut egui::Ui) {
    egui::Panel::bottom("status").exact_size(24.0).frame(egui::Frame::NONE.fill(CHROME)).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            ui.add_space(8.0);
            let text = if viewer.is_open() { viewer.status() } else { "Drop a DWG, DXF or .cad file on the window".to_string() };
            ui.label(egui::RichText::new(text).monospace().color(TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(format!("{}  ·  F fit  G grid  P paper", viewer.units)).color(TEXT));
            });
        });
    });
    let canvas = if viewer.options.paper { PAPER } else { CANVAS };
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(canvas)).show(ui, |ui| {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        viewer.set_size([rect.width(), rect.height()]);
        let origin = rect.min;
        let local = |p: Pos2| [p.x - origin.x, p.y - origin.y];
        let hovered = response.hover_pos().filter(|_| !response.dragged());

        // Pan with the middle button, or the left.
        if response.dragged_by(egui::PointerButton::Middle) || response.dragged_by(egui::PointerButton::Primary) {
            let d = response.drag_delta();
            viewer.pan(d.x, d.y);
        }
        // The wheel zooms about the pointer.
        if response.hovered() {
            let points = ui.input(|i| wheel_points(&i.raw.events));
            if let Some(p) = response.hover_pos() {
                viewer.wheel(f64::from(points), local(p));
            }
        }
        viewer.hover_at(hovered.map(local));
        let keys = ui.input(|i| (i.key_pressed(egui::Key::F) || i.key_pressed(egui::Key::Home), i.key_pressed(egui::Key::G), i.key_pressed(egui::Key::P)));
        if keys.0 {
            viewer.zoom_extents();
        }
        if keys.1 {
            viewer.show_grid = !viewer.show_grid;
        }
        if keys.2 {
            viewer.options.paper = !viewer.options.paper;
        }

        let painter = ui.painter_at(rect);
        let at = |p: [f32; 2]| Pos2::new(origin.x + p[0], origin.y + p[1]);
        paint_grid(viewer, &painter, rect, &at);
        for p in viewer.prims() {
            paint_prim(&painter, &p, &at, None);
        }
        for p in viewer.hover_prims() {
            paint_prim(&painter, &p, &at, Some(HOVER));
        }
        if let Some(s) = viewer.hover.as_ref().and_then(|h| h.snap.as_ref()) {
            let c = at(viewer.view.to_screen(s.point));
            painter.rect_stroke(Rect::from_center_size(c, egui::Vec2::splat(10.0)), 0.0, Stroke::new(1.5, SNAP), egui::StrokeKind::Middle);
            painter.text(c + egui::vec2(10.0, -10.0), Align2::LEFT_BOTTOM, &s.name, FontId::proportional(12.0), SNAP);
        }
        if !viewer.is_open() {
            painter.text(rect.center(), Align2::CENTER_CENTER, "BlackCAD", FontId::proportional(28.0), GRID_MAJOR);
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    });
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
