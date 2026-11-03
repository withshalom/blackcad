//! The view: where the drawing is on the screen.

/// A pan and zoom of the drawing in a canvas. World Y is up; screen Y is down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// The world point at the middle of the canvas.
    pub center: [f64; 2],
    /// Screen points per world unit.
    pub scale: f64,
    /// The canvas size in points.
    pub size: [f32; 2],
}

pub const MIN_SCALE: f64 = 1e-9;
pub const MAX_SCALE: f64 = 1e9;

impl Default for View {
    fn default() -> View {
        View { center: [0.0, 0.0], scale: 1.0, size: [800.0, 600.0] }
    }
}

impl View {
    pub fn new(size: [f32; 2]) -> View {
        View { size, ..View::default() }
    }

    fn half(&self) -> [f64; 2] {
        [f64::from(self.size[0]) / 2.0, f64::from(self.size[1]) / 2.0]
    }

    /// Canvas point (from its top-left) to world.
    pub fn to_world(&self, p: [f32; 2]) -> [f64; 2] {
        let h = self.half();
        [self.center[0] + (f64::from(p[0]) - h[0]) / self.scale, self.center[1] - (f64::from(p[1]) - h[1]) / self.scale]
    }

    /// World point to a canvas point (from its top-left).
    pub fn to_screen(&self, w: [f64; 2]) -> [f32; 2] {
        let h = self.half();
        [((w[0] - self.center[0]) * self.scale + h[0]) as f32, (h[1] - (w[1] - self.center[1]) * self.scale) as f32]
    }

    /// The world rectangle on show: `[x0, y0, x1, y1]`.
    pub fn visible(&self) -> [f64; 4] {
        let h = self.half();
        let (hw, hh) = (h[0] / self.scale, h[1] / self.scale);
        [self.center[0] - hw, self.center[1] - hh, self.center[0] + hw, self.center[1] + hh]
    }

    /// Zoom by `factor` keeping the world point under `at` (canvas point) where it is.
    pub fn zoom_at(&mut self, at: [f32; 2], factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let before = self.to_world(at);
        self.scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let after = self.to_world(at);
        self.center[0] += before[0] - after[0];
        self.center[1] += before[1] - after[1];
    }

    /// Move the drawing by (`dx`, `dy`) canvas points (a drag).
    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        self.center[0] -= f64::from(dx) / self.scale;
        self.center[1] += f64::from(dy) / self.scale;
    }

    /// Show `bounds` (`[x0, y0, x1, y1]`) whole with `margin` points around it.
    pub fn fit(&mut self, bounds: [f64; 4], margin: f32) {
        let (w, h) = (bounds[2] - bounds[0], bounds[3] - bounds[1]);
        if !(bounds.iter().all(|v| v.is_finite()) && w >= 0.0 && h >= 0.0) {
            return;
        }
        self.center = [(bounds[0] + bounds[2]) / 2.0, (bounds[1] + bounds[3]) / 2.0];
        let avail = [f64::from((self.size[0] - 2.0 * margin).max(1.0)), f64::from((self.size[1] - 2.0 * margin).max(1.0))];
        // A single point (no extent) keeps the current scale.
        let s = match (w > 0.0, h > 0.0) {
            (true, true) => (avail[0] / w).min(avail[1] / h),
            (true, false) => avail[0] / w,
            (false, true) => avail[1] / h,
            (false, false) => self.scale,
        };
        self.scale = s.clamp(MIN_SCALE, MAX_SCALE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_and_screen_round_trip() {
        let v = View { center: [100.0, -50.0], scale: 2.5, size: [1000.0, 800.0] };
        for p in [[0.0, 0.0], [500.0, 400.0], [999.0, 799.0]] {
            let w = v.to_world(p);
            let s = v.to_screen(w);
            assert!((s[0] - p[0]).abs() < 1e-3 && (s[1] - p[1]).abs() < 1e-3);
        }
        // the middle of the canvas is the centre; up on screen is up in the world
        assert_eq!(v.to_world([500.0, 400.0]), [100.0, -50.0]);
        assert!(v.to_world([500.0, 0.0])[1] > v.to_world([500.0, 800.0])[1]);
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor() {
        let mut v = View { center: [3.0, 4.0], scale: 10.0, size: [640.0, 480.0] };
        let at = [100.0, 50.0];
        let w0 = v.to_world(at);
        v.zoom_at(at, 1.7);
        let w1 = v.to_world(at);
        assert!((w0[0] - w1[0]).abs() < 1e-9 && (w0[1] - w1[1]).abs() < 1e-9);
        assert!((v.scale - 17.0).abs() < 1e-9);
        v.zoom_at(at, f64::NAN);
        v.zoom_at(at, -1.0);
        assert!((v.scale - 17.0).abs() < 1e-9);
        for _ in 0..1000 {
            v.zoom_at(at, 10.0);
        }
        assert_eq!(v.scale, MAX_SCALE);
    }

    #[test]
    fn pan_moves_the_drawing_with_the_pointer() {
        let mut v = View::new([100.0, 100.0]);
        v.scale = 2.0;
        let p = v.to_screen([5.0, 5.0]);
        v.pan_by(10.0, -4.0);
        let q = v.to_screen([5.0, 5.0]);
        assert!((q[0] - p[0] - 10.0).abs() < 1e-4 && (q[1] - p[1] + 4.0).abs() < 1e-4);
    }

    #[test]
    fn fit_shows_the_whole_drawing() {
        let mut v = View::new([1000.0, 500.0]);
        v.fit([0.0, 0.0, 200.0, 100.0], 10.0);
        let vis = v.visible();
        assert!(vis[0] <= 0.0 && vis[1] <= 0.0 && vis[2] >= 200.0 && vis[3] >= 100.0);
        assert_eq!(v.center, [100.0, 50.0]);
        let s = v.scale;
        v.fit([f64::NAN, 0.0, 1.0, 1.0], 0.0);
        assert_eq!(v.scale, s);
        v.fit([5.0, 5.0, 5.0, 5.0], 0.0); // a point: centre only
        assert_eq!(v.center, [5.0, 5.0]);
        assert_eq!(v.scale, s);
        v.fit([0.0, 0.0, 10.0, 0.0], 0.0); // flat
        assert!(v.scale > 0.0 && v.scale.is_finite());
    }
}
