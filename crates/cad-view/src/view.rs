//! The view: which part of the drawing the viewport shows, and the
//! transform between world and screen.
//!
//! Screen coordinates are in points, from the viewport's top-left corner,
//! y down. The shape of this is CADCraft's `Xf` (crates/ui-egui/src/canvas.rs,
//! MIT OR Apache-2.0; see NOTICE): a centre in the world and a scale in
//! points per drawing unit.

use crate::display::Bounds;

/// Notches of the mouse wheel zoom by this factor (a line is 40 points).
pub const WHEEL_STEP: f64 = 1.2;
const WHEEL_POINTS_PER_STEP: f64 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// The world point at the middle of the viewport.
    pub center: [f64; 2],
    /// Points per drawing unit; always finite and within the limits.
    pub scale: f64,
    /// The viewport's size in points.
    pub size: [f32; 2],
}

impl View {
    pub const MIN_SCALE: f64 = 1e-9;
    pub const MAX_SCALE: f64 = 1e9;

    pub fn new(size: [f32; 2]) -> View {
        View {
            center: [0.0, 0.0],
            scale: 1.0,
            size: clean_size(size),
        }
    }

    pub fn set_size(&mut self, size: [f32; 2]) {
        self.size = clean_size(size);
    }

    /// World to screen, in f64 so far-away points stay exact until the
    /// caller clips them.
    pub fn to_screen_f64(&self, p: [f64; 2]) -> [f64; 2] {
        [
            f64::from(self.size[0]) / 2.0 + (p[0] - self.center[0]) * self.scale,
            f64::from(self.size[1]) / 2.0 - (p[1] - self.center[1]) * self.scale,
        ]
    }

    pub fn to_screen(&self, p: [f64; 2]) -> [f32; 2] {
        let s = self.to_screen_f64(p);
        [finite_f32(s[0]), finite_f32(s[1])]
    }

    pub fn to_world(&self, s: [f32; 2]) -> [f64; 2] {
        [
            self.center[0] + (f64::from(s[0]) - f64::from(self.size[0]) / 2.0) / self.scale,
            self.center[1] - (f64::from(s[1]) - f64::from(self.size[1]) / 2.0) / self.scale,
        ]
    }

    /// The world rectangle the viewport shows.
    pub fn world_bounds(&self) -> Bounds {
        let a = self.to_world([0.0, self.size[1]]);
        let b = self.to_world([self.size[0], 0.0]);
        Bounds::new(a[0], a[1], b[0], b[1])
    }

    /// Drag the drawing by `dx`, `dy` points (what a middle-button drag does).
    pub fn pan_points(&mut self, dx: f32, dy: f32) {
        let c = [
            self.center[0] - f64::from(dx) / self.scale,
            self.center[1] + f64::from(dy) / self.scale,
        ];
        if c[0].is_finite() && c[1].is_finite() {
            self.center = c;
        }
    }

    /// Zoom by `factor` (above 1 closer) keeping the world point under the
    /// screen point `at` where it is, as CAD wheel zoom does.
    pub fn zoom_about(&mut self, factor: f64, at: [f32; 2]) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let anchor = self.to_world(at);
        let scale = (self.scale * factor).clamp(Self::MIN_SCALE, Self::MAX_SCALE);
        let c = [
            anchor[0] - (f64::from(at[0]) - f64::from(self.size[0]) / 2.0) / scale,
            anchor[1] + (f64::from(at[1]) - f64::from(self.size[1]) / 2.0) / scale,
        ];
        if c[0].is_finite() && c[1].is_finite() {
            self.scale = scale;
            self.center = c;
        }
    }

    /// Fit `b` in the viewport with `margin` points around it. An empty or
    /// unbounded rectangle leaves the view alone; a single point is centred
    /// at the current scale.
    pub fn zoom_extents(&mut self, b: &Bounds, margin: f32) {
        if b.is_empty() || !b.is_finite() {
            return;
        }
        let avail_w = (f64::from(self.size[0]) - 2.0 * f64::from(margin)).max(1.0);
        let avail_h = (f64::from(self.size[1]) - 2.0 * f64::from(margin)).max(1.0);
        let fit = |avail: f64, extent: f64| (extent > 0.0).then(|| avail / extent);
        let scale = match (fit(avail_w, b.width()), fit(avail_h, b.height())) {
            (Some(a), Some(c)) => a.min(c),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => self.scale,
        };
        let c = b.center();
        self.scale = if scale.is_finite() {
            scale.clamp(Self::MIN_SCALE, Self::MAX_SCALE)
        } else {
            self.scale
        };
        self.center = c;
    }
}

/// The zoom factor for a vertical wheel scroll of `points` (upward is
/// positive): a whole notch zooms by [`WHEEL_STEP`] at once, as CAD does.
/// Anything not a number is no zoom.
pub fn wheel_factor(points: f64) -> f64 {
    if !points.is_finite() {
        return 1.0;
    }
    WHEEL_STEP.powf(points.clamp(-400.0, 400.0) / WHEEL_POINTS_PER_STEP)
}

fn clean_size(size: [f32; 2]) -> [f32; 2] {
    let ok = |v: f32| {
        if v.is_finite() {
            v.clamp(1.0, 1.0e6)
        } else {
            1.0
        }
    };
    [ok(size[0]), ok(size[1])]
}

fn finite_f32(v: f64) -> f32 {
    if v.is_finite() {
        v.clamp(-1.0e7, 1.0e7) as f32
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
    }

    #[test]
    fn world_and_screen_round_trip() {
        let mut v = View::new([800.0, 600.0]);
        v.center = [10.0, -5.0];
        v.scale = 3.5;
        for p in [[0.0, 0.0], [10.0, -5.0], [-123.5, 77.25]] {
            assert!(close(v.to_world(v.to_screen(p)), p), "{p:?}");
        }
        // the centre is the middle, and y points up on the drawing
        assert_eq!(v.to_screen([10.0, -5.0]), [400.0, 300.0]);
        assert!(v.to_screen([10.0, -4.0])[1] < 300.0);
        let b = v.world_bounds();
        assert!(close(b.min, v.to_world([0.0, 600.0])) && close(b.max, v.to_world([800.0, 0.0])));
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor() {
        let mut v = View::new([800.0, 600.0]);
        let at = [123.0, 456.0];
        let before = v.to_world(at);
        for f in [1.2, 0.5, 7.0, 1.0 / 7.0, 3.0] {
            v.zoom_about(f, at);
            assert!(close(v.to_world(at), before), "after ×{f}");
        }
    }

    #[test]
    fn zoom_stops_at_the_limits_and_refuses_nonsense() {
        let mut v = View::new([100.0, 100.0]);
        for _ in 0..200 {
            v.zoom_about(10.0, [50.0, 50.0]);
        }
        assert_eq!(v.scale, View::MAX_SCALE);
        for _ in 0..200 {
            v.zoom_about(0.1, [50.0, 50.0]);
        }
        assert_eq!(v.scale, View::MIN_SCALE);
        let keep = v;
        for f in [f64::NAN, f64::INFINITY, 0.0, -2.0] {
            v.zoom_about(f, [10.0, 10.0]);
            assert_eq!(v, keep);
        }
    }

    #[test]
    fn pan_moves_the_drawing_with_the_pointer() {
        let mut v = View::new([800.0, 600.0]);
        v.scale = 2.0;
        let p = [3.0, 4.0];
        let before = v.to_screen(p);
        v.pan_points(30.0, -12.0);
        let after = v.to_screen(p);
        assert!(
            (after[0] - before[0] - 30.0).abs() < 1e-3
                && (after[1] - before[1] + 12.0).abs() < 1e-3
        );
        let keep = v;
        v.pan_points(f32::NAN, 1.0);
        assert_eq!(v, keep);
    }

    #[test]
    fn extents_fit_with_a_margin() {
        let mut v = View::new([1000.0, 500.0]);
        v.zoom_extents(&Bounds::new(0.0, 0.0, 100.0, 100.0), 50.0);
        assert!(close(v.center, [50.0, 50.0]));
        assert!(
            (v.scale - 4.0).abs() < 1e-9,
            "the height (400 points for 100 units) limits it: {}",
            v.scale
        );
        let b = v.world_bounds();
        assert!(b.min[1] < 0.0 && b.max[1] > 100.0);
        // a flat drawing fits by its width alone
        v.zoom_extents(&Bounds::new(0.0, 5.0, 10.0, 5.0), 0.0);
        assert!((v.scale - 100.0).abs() < 1e-9);
        // a point is centred at the scale it had
        let s = v.scale;
        v.zoom_extents(&Bounds::new(7.0, 8.0, 7.0, 8.0), 10.0);
        assert!(close(v.center, [7.0, 8.0]) && v.scale == s);
        // nothing to fit leaves the view
        let keep = v;
        v.zoom_extents(&Bounds::EMPTY, 10.0);
        v.zoom_extents(&Bounds::EVERYWHERE, 10.0);
        assert_eq!(v, keep);
    }

    #[test]
    fn a_degenerate_viewport_does_not_divide_by_zero() {
        let mut v = View::new([0.0, f32::NAN]);
        assert_eq!(v.size, [1.0, 1.0]);
        v.zoom_extents(&Bounds::new(0.0, 0.0, 1.0, 1.0), 100.0);
        assert!(v.scale.is_finite() && v.scale > 0.0);
    }

    #[test]
    fn far_points_do_not_leave_the_float_range() {
        let v = View::new([100.0, 100.0]);
        assert_eq!(v.to_screen([1e300, -1e300]), [1.0e7, 1.0e7]);
        assert_eq!(v.to_screen([f64::NAN, 0.0])[0], 0.0);
    }

    #[test]
    fn wheel_notches() {
        assert!((wheel_factor(40.0) - WHEEL_STEP).abs() < 1e-12);
        assert!((wheel_factor(-40.0) * WHEEL_STEP - 1.0).abs() < 1e-12);
        assert_eq!(wheel_factor(0.0), 1.0);
        assert_eq!(wheel_factor(f64::NAN), 1.0);
        assert!(wheel_factor(1e12).is_finite());
    }
}
