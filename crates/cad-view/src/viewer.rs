//! Everything a viewer app does that is not toolkit: it owns the session and
//! the open drawing, keeps the view and the display list, answers the
//! pointer (what is under it, the snap point near it) and hands the app the
//! shapes to paint. The egui app in `apps/viewport` only paints these and
//! feeds the mouse and keys in.

use crate::display::{Bounds, Display};
use crate::draw::{DrawOptions, Prim, tessellate, tessellate_item};
use crate::grid::{GRID_SPACING, Grid, grid};
use crate::json::Json;
use crate::query::{SnapHit, parse_pick, parse_snaps, pick_command, snap_command};
use crate::view::{View, wheel_factor};
use cad_ffi::session::Session;

/// How near the pointer must be, in points, for a snap or a pick to take.
pub const APERTURE_POINTS: f64 = 10.0;
/// The snap kinds on at the start: the ones that find defined points
/// (`cad_core::snap::kind::DEFAULT`).
pub const DEFAULT_SNAPS: u32 = 1 | 2 | 4 | 8 | 16 | 64 | 128;
/// Margin, in points, left round the drawing by zoom-to-extents.
pub const EXTENTS_MARGIN: f32 = 24.0;

/// What is under the pointer.
#[derive(Debug, Clone, PartialEq)]
pub struct Hover {
    /// The pointer, in world coordinates.
    pub world: [f64; 2],
    /// The snap point taken, if one is near.
    pub snap: Option<SnapHit>,
    /// The entity under the pointer (the snap's, if there is one).
    pub handle: Option<u64>,
}

/// One layer as the drawing's styles name it.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerEntry {
    pub name: String,
    pub visible: bool,
}

pub struct Viewer {
    session: Session,
    id: Option<u32>,
    pub name: String,
    pub display: Display,
    pub view: View,
    pub options: DrawOptions,
    pub show_grid: bool,
    pub snap_kinds: u32,
    pub hover: Option<Hover>,
    /// The drawing's units as the core names them ("millimetres"...).
    pub units: String,
    /// The last thing that went wrong or worth saying, for the status bar.
    pub message: String,
}

impl Default for Viewer {
    fn default() -> Self {
        Viewer::new()
    }
}

impl Viewer {
    pub fn new() -> Viewer {
        Viewer {
            session: Session::new(),
            id: None,
            name: String::new(),
            display: Display {
                bounds: None,
                styles: Vec::new(),
                items: Vec::new(),
                skipped: Vec::new(),
            },
            view: View::new([800.0, 600.0]),
            options: DrawOptions::default(),
            show_grid: true,
            snap_kinds: DEFAULT_SNAPS,
            hover: None,
            units: String::new(),
            message: String::new(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.id.is_some()
    }

    /// Open a drawing (BlackCAD, DXF or DWG, told apart by content) and fit
    /// it to the view. On failure the drawing that was open stays open.
    pub fn open(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let id = self.session.open(bytes)?;
        self.adopt(id, name)
    }

    /// The sample plate the core ships.
    pub fn open_sample(&mut self) -> Result<(), String> {
        let id = self.session.sample()?;
        self.adopt(id, "sample plate")
    }

    fn adopt(&mut self, id: u32, name: &str) -> Result<(), String> {
        let shown = self
            .session
            .display(id)
            .and_then(|b| Display::parse(&b))
            .inspect_err(|_| self.session.close(id))?;
        if let Some(old) = self.id.replace(id) {
            self.session.close(old);
        }
        self.name = name.to_string();
        self.display = shown;
        self.hover = None;
        self.units = self.read_units(id);
        self.options.hidden_layers.clear();
        self.zoom_extents();
        self.message = format!("{name}: {} objects", self.display.items.len());
        if !self.display.skipped.is_empty() {
            let n: f64 = self.display.skipped.iter().map(|(_, n)| *n).sum();
            self.message.push_str(&format!(", {n} not drawn"));
        }
        Ok(())
    }

    fn read_units(&mut self, id: u32) -> String {
        self.session
            .info(id)
            .ok()
            .and_then(|j| Json::parse(&j).ok())
            .and_then(|j| j.get("units").and_then(Json::as_str).map(str::to_string))
            .unwrap_or_default()
    }

    pub fn set_size(&mut self, size: [f32; 2]) {
        self.view.set_size(size);
    }

    pub fn zoom_extents(&mut self) {
        if let Some(b) = self.display.extents() {
            self.view.zoom_extents(&b, EXTENTS_MARGIN);
        }
    }

    pub fn extents(&self) -> Option<Bounds> {
        self.display.extents()
    }

    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.view.pan_points(dx, dy);
        self.hover = None;
    }

    /// A vertical wheel scroll of `points` (up is positive) about the screen
    /// point `at`.
    pub fn wheel(&mut self, points: f64, at: [f32; 2]) {
        let f = wheel_factor(points);
        if f != 1.0 {
            self.view.zoom_about(f, at);
            self.hover = None;
        }
    }

    pub fn grid(&self) -> Option<Grid> {
        self.show_grid.then(|| grid(&self.view, GRID_SPACING))
    }

    /// The drawing as shapes for the view.
    pub fn prims(&self) -> Vec<Prim> {
        tessellate(&self.display, &self.view, &self.options)
    }

    /// The entity `hover` points at, as shapes (to draw it again, lit).
    pub fn hover_prims(&self) -> Vec<Prim> {
        let mut out = Vec::new();
        if let Some(h) = self.hover.as_ref().and_then(|h| h.handle) {
            for i in self.display.items_of(h) {
                tessellate_item(&self.display, i, &self.view, &self.options, &mut out);
            }
        }
        out
    }

    /// The pointer moved to the screen point `at` (`None`: it left).
    pub fn hover_at(&mut self, at: Option<[f32; 2]>) {
        let (Some(at), Some(id)) = (at, self.id) else {
            self.hover = None;
            return;
        };
        let world = self.view.to_world(at);
        let aperture = APERTURE_POINTS / self.view.scale;
        let snap = self
            .session
            .exec(id, &snap_command(world, aperture, self.snap_kinds))
            .ok()
            .and_then(|r| parse_snaps(&r).ok())
            .and_then(|s| s.into_iter().next());
        let handle = match &snap {
            Some(s) if s.handle != 0 => Some(s.handle),
            _ => self
                .session
                .exec(id, &pick_command(world, aperture))
                .ok()
                .and_then(|r| parse_pick(&r).ok())
                .flatten(),
        };
        self.hover = Some(Hover {
            world,
            snap,
            handle,
        });
    }

    /// The layers named by the styles, once each, with whether each shows.
    pub fn layers(&self) -> Vec<LayerEntry> {
        let mut out: Vec<LayerEntry> = Vec::new();
        for s in &self.display.styles {
            if !out.iter().any(|l| l.name == s.layer) {
                out.push(LayerEntry {
                    visible: !self.options.hidden_layers.contains(&s.layer),
                    name: s.layer.clone(),
                });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn set_layer_visible(&mut self, name: &str, visible: bool) {
        self.options.hidden_layers.retain(|l| l != name);
        if !visible {
            self.options.hidden_layers.push(name.to_string());
        }
    }

    /// The status line: the pointer's drawing coordinates and what it is on.
    pub fn status(&self) -> String {
        match &self.hover {
            Some(h) => {
                let p = h.snap.as_ref().map_or(h.world, |s| s.point);
                let mut s = format!("X {:.3}  Y {:.3}", p[0], p[1]);
                if let Some(k) = &h.snap {
                    s.push_str(&format!("  {}", k.name));
                }
                if let Some(e) = h.handle {
                    s.push_str(&format!("  entity {e:X}"));
                }
                s
            }
            None => self.message.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Viewer {
        let mut v = Viewer::new();
        v.set_size([1000.0, 700.0]);
        v.open_sample().unwrap();
        v
    }

    impl Viewer {
        fn new_with_sample_items() -> usize {
            sample().display.items.len()
        }
    }

    #[test]
    fn opens_the_sample_and_fits_it() {
        let v = sample();
        assert!(v.is_open() && !v.display.items.is_empty());
        let ext = v.extents().unwrap();
        // the drawing sits inside the view after zoom-to-extents
        let wb = v.view.world_bounds();
        assert!(wb.min[0] <= ext.min[0] && wb.max[0] >= ext.max[0]);
        assert!(wb.min[1] <= ext.min[1] && wb.max[1] >= ext.max[1]);
        assert!(!v.units.is_empty(), "units read from info");
        assert!(v.message.contains("objects"), "{}", v.message);
        assert!(!v.prims().is_empty());
    }

    #[test]
    fn a_bad_file_leaves_the_open_drawing_alone() {
        let mut v = sample();
        let (name, n) = (v.name.clone(), v.display.items.len());
        assert!(v.open("junk.dwg", b"not a drawing").is_err());
        assert!(v.open("empty.dxf", b"").is_err());
        assert_eq!((v.name.clone(), v.display.items.len()), (name, n));
        assert!(v.is_open());
    }

    #[test]
    fn a_bad_file_with_nothing_open_stays_closed() {
        let mut v = Viewer::new();
        assert!(v.open("junk", &[0u8; 64]).is_err());
        assert!(!v.is_open());
        assert!(v.prims().is_empty() && v.hover_prims().is_empty());
        v.hover_at(Some([5.0, 5.0]));
        assert_eq!(v.hover, None);
        assert_eq!(v.status(), "");
    }

    #[test]
    fn pointing_at_a_corner_snaps_to_it() {
        let mut v = sample();
        // the start of the first line the drawing has: a defined point
        let p = v
            .display
            .items
            .iter()
            .flat_map(|i| i.ops.iter())
            .find_map(|o| match o {
                crate::display::Op::Line(l) => Some([l[0], l[1]]),
                _ => None,
            })
            .expect("the sample has a line");
        let at = v.view.to_screen(p);
        v.hover_at(Some([at[0] + 2.0, at[1] - 2.0]));
        let h = v.hover.clone().expect("hover");
        assert!(h.snap.is_some() && h.handle.is_some(), "{h:?}");
        assert!(v.status().starts_with("X "), "{}", v.status());
        let lit = v.hover_prims();
        if let Some(e) = h.handle {
            assert!(v.display.items_of(e).count() > 0);
            assert!(
                !lit.is_empty(),
                "the entity under the pointer is drawn again"
            );
            assert!(lit.iter().all(|p| v.display.items[p.item].handle == e));
        }
        v.hover_at(None);
        assert_eq!(v.hover, None);
    }

    #[test]
    fn pointing_at_nothing_hovers_with_no_entity() {
        let mut v = sample();
        // far outside the drawing but still in the view
        v.view.zoom_about(1.0e-3, [500.0, 350.0]);
        v.hover_at(Some([3.0, 3.0]));
        let h = v.hover.clone().unwrap();
        assert_eq!((h.snap, h.handle), (None, None));
    }

    #[test]
    fn pan_and_wheel_move_the_view_and_clear_the_hover() {
        let mut v = sample();
        v.hover_at(Some([100.0, 100.0]));
        assert!(v.hover.is_some());
        let before = v.view;
        v.pan(10.0, 0.0);
        assert!(v.hover.is_none() && v.view.center != before.center);
        v.hover_at(Some([100.0, 100.0]));
        v.wheel(40.0, [500.0, 350.0]);
        assert!(v.view.scale > before.scale && v.hover.is_none());
        let s = v.view.scale;
        v.wheel(f64::NAN, [500.0, 350.0]);
        assert_eq!(v.view.scale, s);
        v.zoom_extents();
        assert!((v.view.scale - before.scale).abs() / before.scale < 1e-9);
    }

    #[test]
    fn layers_list_and_hide() {
        let mut v = sample();
        let layers = v.layers();
        assert!(!layers.is_empty() && layers.iter().all(|l| l.visible));
        let all = v.prims().len();
        for l in &layers {
            v.set_layer_visible(&l.name, false);
        }
        assert!(v.prims().is_empty(), "every layer off draws nothing");
        assert!(v.layers().iter().all(|l| !l.visible));
        v.set_layer_visible(&layers[0].name, true);
        assert!(v.layers()[0].visible);
        for l in &layers {
            v.set_layer_visible(&l.name, true);
        }
        assert_eq!(v.prims().len(), all);
        // switching a layer back on twice does not duplicate anything
        v.set_layer_visible(&layers[0].name, false);
        v.set_layer_visible(&layers[0].name, false);
        assert_eq!(v.options.hidden_layers.len(), 1);
    }

    #[test]
    fn the_grid_follows_the_switch() {
        let mut v = sample();
        assert!(v.grid().is_some());
        v.show_grid = false;
        assert!(v.grid().is_none());
    }

    #[test]
    fn opens_what_the_core_saves_in_each_format() {
        let mut s = Session::new();
        let id = s.sample().unwrap();
        let items = Viewer::new_with_sample_items();
        for format in ["cad", "dxf", "dwg"] {
            let (bytes, _) = s.save(id, format, 0, "test").unwrap();
            let mut v = Viewer::new();
            v.open(&format!("plate.{format}"), &bytes).unwrap();
            assert!(v.is_open(), "{format}");
            assert!(
                v.display.items.len() * 2 >= items,
                "{format}: {} of {items} objects",
                v.display.items.len()
            );
        }
    }
}
