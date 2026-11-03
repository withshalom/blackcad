//! Finding the entity under a point.

use crate::parse::{Display, Op};

fn seg_dist2(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let (qx, qy) = (a[0] + t * dx, a[1] + t * dy);
    (p[0] - qx).powi(2) + (p[1] - qy).powi(2)
}

fn arc_dist2(p: [f64; 2], c: [f64; 2], r: f64, a0: f64, a1: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    let sweep = if a1 - a0 >= tau - 1e-12 { tau } else { (a1 - a0).rem_euclid(tau) };
    let ang = (p[1] - c[1]).atan2(p[0] - c[0]);
    let rel = (ang - a0).rem_euclid(tau);
    let d_c = ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt();
    if rel <= sweep {
        return (d_c - r).powi(2);
    }
    // Off the end of the arc: the nearer endpoint.
    let e = |a: f64| [c[0] + r * a.cos(), c[1] + r * a.sin()];
    let (s, t) = (e(a0), e(a0 + sweep));
    ((p[0] - s[0]).powi(2) + (p[1] - s[1]).powi(2)).min((p[0] - t[0]).powi(2) + (p[1] - t[1]).powi(2))
}

impl Display {
    /// The handle of the item whose drawing passes within `tol` (world units) of `p`, the
    /// nearest if several do. A fill counts when `p` is inside it, but a stroke nearer than
    /// any fill wins, so a line on a hatch can still be picked.
    pub fn pick(&self, p: [f64; 2], tol: f64) -> Option<u64> {
        if !(p[0].is_finite() && p[1].is_finite() && tol.is_finite() && tol >= 0.0) {
            return None;
        }
        let mut best: Option<(f64, u64)> = None;
        let mut best_fill: Option<u64> = None;
        for it in &self.items {
            let b = it.bbox;
            if b[0] > b[2] || p[0] < b[0] - tol || p[0] > b[2] + tol || p[1] < b[1] - tol || p[1] > b[3] + tol {
                continue;
            }
            let mut d2 = f64::INFINITY;
            for op in self.item_ops(it) {
                match op {
                    Op::Line(l) => d2 = d2.min(seg_dist2(p, [l[0], l[1]], [l[2], l[3]])),
                    Op::Arc { c, r, a0, a1 } => d2 = d2.min(arc_dist2(p, c, r, a0, a1)),
                    Op::Path { pts, closed } => {
                        let v: Vec<[f64; 2]> = pts.chunks_exact(2).map(|q| [q[0], q[1]]).collect();
                        for w in v.windows(2) {
                            d2 = d2.min(seg_dist2(p, w[0], w[1]));
                        }
                        if closed && v.len() > 2 {
                            d2 = d2.min(seg_dist2(p, v[v.len() - 1], v[0]));
                        }
                        if v.len() == 1 {
                            d2 = d2.min((p[0] - v[0][0]).powi(2) + (p[1] - v[0][1]).powi(2));
                        }
                    }
                    Op::Dot(q) => d2 = d2.min((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)),
                    Op::Fill { rings } => {
                        // Even-odd inside test over every ring.
                        let mut inside = false;
                        for r in rings {
                            let v: Vec<[f64; 2]> = r.chunks_exact(2).map(|q| [q[0], q[1]]).collect();
                            let mut j = v.len().wrapping_sub(1);
                            for i in 0..v.len() {
                                let (a, c) = (v[i], v[j]);
                                if (a[1] > p[1]) != (c[1] > p[1]) && p[0] < (c[0] - a[0]) * (p[1] - a[1]) / (c[1] - a[1]) + a[0] {
                                    inside = !inside;
                                }
                                j = i;
                            }
                        }
                        if inside {
                            best_fill = Some(it.handle);
                        }
                    }
                    Op::Ray { o, d, ray, .. } => {
                        // Construction lines have no extent, so they are tested here directly.
                        let t = (p[0] - o[0]) * d[0] + (p[1] - o[1]) * d[1];
                        let t = if ray { t.max(0.0) } else { t };
                        let q = [o[0] + t * d[0], o[1] + t * d[1]];
                        d2 = d2.min((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2));
                    }
                }
            }
            if d2.sqrt() <= tol && best.is_none_or(|(bd, _)| d2 < bd) {
                best = Some((d2, it.handle));
            }
        }
        best.map(|(_, h)| h).or(best_fill)
    }
}

#[cfg(test)]
mod tests {
    use crate::parse::tests::{encode, HEAD};
    use crate::Display;

    #[test]
    fn picks_the_nearest_stroke_within_tolerance() {
        let d = Display::parse(&encode(
            HEAD,
            &[
                1.0, 10.0, 0.0, 2.0, 0.0, 0.0, 10.0, 0.0, // y = 0
                1.0, 11.0, 0.0, 2.0, 0.0, 1.0, 10.0, 1.0, // y = 1
                1.0, 12.0, 0.0, 3.0, 20.0, 0.0, 5.0, 0.0, std::f64::consts::PI, // upper half circle at (20,0) r5
            ],
        ))
        .unwrap();
        assert_eq!(d.pick([5.0, 0.2], 0.5), Some(10));
        assert_eq!(d.pick([5.0, 0.8], 0.5), Some(11));
        assert_eq!(d.pick([5.0, 0.5], 0.6), Some(10).or(Some(11)));
        assert_eq!(d.pick([5.0, 3.0], 0.5), None);
        // on the arc (top of the circle), not on its missing lower half
        assert_eq!(d.pick([20.0, 5.1], 0.3), Some(12));
        assert_eq!(d.pick([20.0, -5.0], 0.3), None);
        assert_eq!(d.pick([f64::NAN, 0.0], 1.0), None);
    }

    #[test]
    fn a_fill_is_picked_inside_but_a_stroke_wins() {
        let d = Display::parse(&encode(
            HEAD,
            &[
                1.0, 20.0, 0.0, 5.0, 2.0, 4.0, 0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0, 4.0, 3.0, 3.0, 6.0, 3.0, 6.0, 6.0, 3.0, 6.0,
                1.0, 21.0, 0.0, 2.0, 0.0, 5.0, 10.0, 5.0,
            ],
        ))
        .unwrap();
        assert_eq!(d.pick([1.0, 1.0], 0.1), Some(20)); // in the solid part
        assert_eq!(d.pick([4.5, 4.5], 0.1), None); // in the hole
        assert_eq!(d.pick([2.0, 5.0], 0.2), Some(21)); // the line across it wins
    }
}
