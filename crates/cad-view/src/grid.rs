//! The grid and the axes, as lines in screen coordinates.
//!
//! Spacing follows the web workbench (DESIGN.md §2): steps of five so the
//! lines stay 8 to 40 points apart, every fifth line a major one. The line
//! count is capped with checked arithmetic, as CADCraft's grid is
//! (crates/ui-egui/src/canvas/grid.rs, MIT OR Apache-2.0; see NOTICE): a view
//! that is absurdly large saturates the float-to-integer casts, and
//! subtracting those would overflow.

use crate::display::Bounds;
use crate::view::View;

/// The grid's base spacing in drawing units (`standards::canvas::GRID_SPACING`).
pub const GRID_SPACING: f64 = 10.0;
/// More lines than this along an axis and the grid is not drawn.
pub const MAX_LINES: i64 = 2000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLine {
    pub a: [f32; 2],
    pub b: [f32; 2],
    pub major: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Grid {
    pub lines: Vec<GridLine>,
    /// Where the line y = 0 (the X axis) falls on the screen, if it does.
    pub x_axis: Option<f32>,
    /// Where the line x = 0 (the Y axis) falls on the screen, if it does.
    pub y_axis: Option<f32>,
}

/// The spacing to draw: `base` times a power of five that puts the lines 8
/// to 40 points apart.
pub fn step(base: f64, scale: f64) -> f64 {
    if !(base.is_finite() && base > 0.0 && scale.is_finite() && scale > 0.0) {
        return GRID_SPACING;
    }
    let mut g = base;
    let mut guard = 0;
    while g * scale < 8.0 && guard < 400 {
        g *= 5.0;
        guard += 1;
    }
    while g * scale >= 40.0 && guard < 800 {
        g /= 5.0;
        guard += 1;
    }
    g
}

/// Index ranges of the grid lines inside `wb` for line spacing `g`, or
/// `None` when there would be `MAX_LINES` or more along an axis.
pub fn index_ranges(wb: &Bounds, g: f64) -> Option<((i64, i64), (i64, i64))> {
    if !(g.is_finite() && g > 0.0) || !wb.is_finite() {
        return None;
    }
    let range = |lo: f64, hi: f64| {
        let a = (lo / g).floor() as i64;
        let b = (hi / g).ceil() as i64;
        b.checked_sub(a).filter(|n| *n < MAX_LINES).map(|_| (a, b))
    };
    Some((range(wb.min[0], wb.max[0])?, range(wb.min[1], wb.max[1])?))
}

/// The grid lines and axes for `view`. `base` is the unit the grid steps by.
pub fn grid(view: &View, base: f64) -> Grid {
    let mut out = Grid::default();
    let [w, h] = view.size;
    let o = view.to_screen_f64([0.0, 0.0]);
    if o[1] >= 0.0 && o[1] <= f64::from(h) {
        out.x_axis = Some(o[1] as f32);
    }
    if o[0] >= 0.0 && o[0] <= f64::from(w) {
        out.y_axis = Some(o[0] as f32);
    }
    let g = step(base, view.scale);
    let Some(((x0, x1), (y0, y1))) = index_ranges(&view.world_bounds(), g) else {
        return out;
    };
    // Lines on whole pixels, drawn between pixel centres, stay sharp.
    let crisp = |v: f64| (v.round() + 0.5) as f32;
    for i in x0..=x1 {
        let sx = crisp(view.to_screen_f64([i as f64 * g, 0.0])[0]);
        out.lines.push(GridLine {
            a: [sx, 0.0],
            b: [sx, h],
            major: i % 5 == 0,
        });
    }
    for j in y0..=y1 {
        let sy = crisp(view.to_screen_f64([0.0, j as f64 * g])[1]);
        out.lines.push(GridLine {
            a: [0.0, sy],
            b: [w, sy],
            major: j % 5 == 0,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacing_keeps_lines_between_8_and_40_points() {
        for scale in [1e-6, 0.01, 0.3, 1.0, 2.5, 40.0, 1e4, 1e8] {
            let g = step(GRID_SPACING, scale);
            let px = g * scale;
            assert!(
                (8.0..40.0).contains(&px) || (px - 40.0).abs() < 1e-9,
                "scale {scale}: {px}"
            );
        }
        assert_eq!(step(f64::NAN, 1.0), GRID_SPACING);
        assert_eq!(step(10.0, 0.0), GRID_SPACING);
    }

    #[test]
    fn index_ranges_survive_extreme_views() {
        // CADCraft's own cases (canvas/grid.rs tests).
        let huge = Bounds::new(-1e300, -1e300, 1e300, 1e300);
        assert_eq!(index_ranges(&huge, 1.0), None);
        let normal = Bounds::new(-5.0, 0.0, 5.0, 10.0);
        assert_eq!(index_ranges(&normal, 1.0), Some(((-5, 5), (0, 10))));
        assert_eq!(index_ranges(&normal, 0.0), None);
        assert_eq!(index_ranges(&Bounds::EVERYWHERE, 1.0), None);
    }

    #[test]
    fn the_grid_covers_the_view_with_a_major_line_every_fifth() {
        let mut v = View::new([400.0, 300.0]);
        v.scale = 2.0; // 10 units = 20 points
        let g = grid(&v, GRID_SPACING);
        let verticals: Vec<_> = g.lines.iter().filter(|l| l.a[0] == l.b[0]).collect();
        let horizontals = g.lines.len() - verticals.len();
        assert!(verticals.len() >= 400 / 20 && horizontals >= 300 / 20);
        assert!(g.lines.iter().filter(|l| l.major).count() * 5 <= g.lines.len() + 10);
        // the origin is in view: both axes show, at the middle
        assert_eq!((g.x_axis, g.y_axis), (Some(150.0), Some(200.0)));
        // x = 0 is a major vertical line
        let at_origin = verticals
            .iter()
            .find(|l| (l.a[0] - 200.5).abs() < 1e-3)
            .expect("a line at the origin");
        assert!(at_origin.major);
    }

    #[test]
    fn axes_leave_when_the_origin_does() {
        let mut v = View::new([100.0, 100.0]);
        v.center = [1000.0, 1000.0];
        let g = grid(&v, GRID_SPACING);
        assert_eq!((g.x_axis, g.y_axis), (None, None));
    }

    #[test]
    fn an_absurd_view_draws_no_grid_and_does_not_panic() {
        let mut v = View::new([100.0, 100.0]);
        v.scale = View::MIN_SCALE;
        v.center = [1e300, 1e300];
        assert!(grid(&v, GRID_SPACING).lines.len() < 4000);
        v.center = [f64::MAX, f64::MAX];
        let _ = grid(&v, GRID_SPACING);
    }
}
