//! The drawing tools of the viewport: what each click means, the command
//! line each tool ends in, and the rubber band shown while it waits for its
//! next point. No toolkit and no session here: points in, command text out.

use crate::display::Op;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    /// Click an object to select it.
    #[default]
    Select,
    Line,
    Circle,
    Rectangle,
    Polyline,
    /// An arc through three points.
    Arc,
    /// Click an object to erase it.
    Erase,
}

impl Tool {
    pub const ALL: [Tool; 7] = [
        Tool::Select,
        Tool::Line,
        Tool::Circle,
        Tool::Rectangle,
        Tool::Polyline,
        Tool::Arc,
        Tool::Erase,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Line => "Line",
            Tool::Circle => "Circle",
            Tool::Rectangle => "Rectangle",
            Tool::Polyline => "Polyline",
            Tool::Arc => "Arc",
            Tool::Erase => "Erase",
        }
    }

    /// The command-line word, and the short one, that starts the tool.
    pub fn from_word(w: &str) -> Option<Tool> {
        Some(match w.to_ascii_lowercase().as_str() {
            "select" | "sel" | "esc" => Tool::Select,
            "line" | "l" => Tool::Line,
            "circle" | "c" | "ci" => Tool::Circle,
            "rect" | "rectangle" | "rec" => Tool::Rectangle,
            "pline" | "polyline" | "pl" => Tool::Polyline,
            "arc" | "a" | "arc-3p" => Tool::Arc,
            "erase" | "e" | "delete" | "del" => Tool::Erase,
            _ => return None,
        })
    }

    /// Whether the tool takes points (rather than a click on an object).
    pub fn takes_points(self) -> bool {
        !matches!(self, Tool::Select | Tool::Erase)
    }

    /// What the tool waits for, given the points it has.
    pub fn prompt(self, have: usize) -> &'static str {
        match (self, have) {
            (Tool::Select, _) => "Click an object to select it; Delete erases it",
            (Tool::Erase, _) => "Click the object to erase",
            (Tool::Line, 0) => "Line: click the start point",
            (Tool::Line, _) => "Line: click the next point (Esc ends)",
            (Tool::Circle, 0) => "Circle: click the centre",
            (Tool::Circle, _) => "Circle: click a point on it, or type the radius",
            (Tool::Rectangle, 0) => "Rectangle: click the first corner",
            (Tool::Rectangle, _) => "Rectangle: click the opposite corner",
            (Tool::Polyline, 0) => "Polyline: click the start point",
            (Tool::Polyline, _) => {
                "Polyline: click the next point (Enter or right-click ends, C closes)"
            }
            (Tool::Arc, 0) => "Arc: click the start point",
            (Tool::Arc, 1) => "Arc: click a point on the arc",
            (Tool::Arc, _) => "Arc: click the end point",
        }
    }
}

fn num(v: f64) -> String {
    if v.is_finite() {
        format!("{v}")
    } else {
        "0".into()
    }
}

/// A point `p` for `tool`, which has `points` so far. Returns the command
/// to run when the point completes the object. A line carries on from its
/// end; the others start over.
pub fn click(tool: Tool, points: &mut Vec<[f64; 2]>, p: [f64; 2]) -> Option<String> {
    if !(p[0].is_finite() && p[1].is_finite()) {
        return None;
    }
    match tool {
        Tool::Select | Tool::Erase => None,
        Tool::Line => {
            let Some(&a) = points.last() else {
                points.push(p);
                return None;
            };
            if a == p {
                return None;
            }
            *points = vec![p];
            Some(format!(
                "line {} {} {} {}",
                num(a[0]),
                num(a[1]),
                num(p[0]),
                num(p[1])
            ))
        }
        Tool::Circle => {
            let Some(&c) = points.first() else {
                points.push(p);
                return None;
            };
            points.clear();
            circle(c, (p[0] - c[0]).hypot(p[1] - c[1]))
        }
        Tool::Rectangle => {
            let Some(&a) = points.first() else {
                points.push(p);
                return None;
            };
            if a[0] == p[0] || a[1] == p[1] {
                return None;
            }
            points.clear();
            Some(format!(
                "rect {} {} {} {}",
                num(a[0]),
                num(a[1]),
                num(p[0]),
                num(p[1])
            ))
        }
        Tool::Polyline => {
            if points.last() != Some(&p) {
                points.push(p);
            }
            None
        }
        Tool::Arc => {
            points.push(p);
            if points.len() < 3 {
                return None;
            }
            let q = std::mem::take(points);
            let (a, b, c) = (q[0], q[1], q[2]);
            // three points on a line make no arc
            let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if cross == 0.0 {
                return None;
            }
            Some(format!(
                "arc-3p {} {} {} {} {} {}",
                num(a[0]),
                num(a[1]),
                num(b[0]),
                num(b[1]),
                num(c[0]),
                num(c[1])
            ))
        }
    }
}

/// The circle command for centre `c` and radius `r`; none for no radius.
pub fn circle(c: [f64; 2], r: f64) -> Option<String> {
    (r.is_finite() && r > 0.0).then(|| format!("circle {} {} {}", num(c[0]), num(c[1]), num(r)))
}

/// The polyline the points make, closed or open; none under two points.
pub fn polyline(points: &[[f64; 2]], closed: bool) -> Option<String> {
    if points.len() < 2 {
        return None;
    }
    let mut s = format!("pline {}", u8::from(closed));
    for p in points {
        s.push_str(&format!(" {} {} 0", num(p[0]), num(p[1])));
    }
    Some(s)
}

/// The rubber band: what the tool would draw if the next point were `to`.
pub fn preview(tool: Tool, points: &[[f64; 2]], to: [f64; 2]) -> Vec<Op> {
    if !(to[0].is_finite() && to[1].is_finite()) {
        return Vec::new();
    }
    let path = |pts: Vec<[f64; 2]>, closed: bool| {
        vec![Op::Path {
            closed,
            points: pts,
        }]
    };
    match (tool, points) {
        (Tool::Line, [.., a]) => vec![Op::Line([a[0], a[1], to[0], to[1]])],
        (Tool::Circle, [c]) => {
            let r = (to[0] - c[0]).hypot(to[1] - c[1]);
            if r > 0.0 {
                vec![Op::Arc {
                    center: *c,
                    radius: r,
                    start: 0.0,
                    end: std::f64::consts::TAU,
                }]
            } else {
                Vec::new()
            }
        }
        (Tool::Rectangle, [a]) => path(vec![*a, [to[0], a[1]], to, [a[0], to[1]]], true),
        (Tool::Polyline, pts) if !pts.is_empty() => {
            let mut v = pts.to_vec();
            v.push(to);
            path(v, false)
        }
        (Tool::Arc, pts) if !pts.is_empty() => {
            let mut v = pts.to_vec();
            v.push(to);
            path(v, false)
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_chains_from_its_end() {
        let mut pts = Vec::new();
        assert_eq!(click(Tool::Line, &mut pts, [0.0, 0.0]), None);
        assert_eq!(
            click(Tool::Line, &mut pts, [10.0, 0.0]).unwrap(),
            "line 0 0 10 0"
        );
        assert_eq!(pts, vec![[10.0, 0.0]]);
        assert_eq!(
            click(Tool::Line, &mut pts, [10.0, 5.5]).unwrap(),
            "line 10 0 10 5.5"
        );
        // the same point twice draws nothing
        assert_eq!(click(Tool::Line, &mut pts, [10.0, 5.5]), None);
    }

    #[test]
    fn circles_rectangles_and_arcs_start_over() {
        let mut pts = Vec::new();
        click(Tool::Circle, &mut pts, [1.0, 1.0]);
        assert_eq!(
            click(Tool::Circle, &mut pts, [4.0, 5.0]).unwrap(),
            "circle 1 1 5"
        );
        assert!(pts.is_empty());
        click(Tool::Circle, &mut pts, [1.0, 1.0]);
        assert_eq!(click(Tool::Circle, &mut pts, [1.0, 1.0]), None, "no radius");
        assert!(pts.is_empty());

        click(Tool::Rectangle, &mut pts, [0.0, 0.0]);
        assert_eq!(
            click(Tool::Rectangle, &mut pts, [0.0, 3.0]),
            None,
            "no area"
        );
        assert_eq!(pts.len(), 1);
        assert_eq!(
            click(Tool::Rectangle, &mut pts, [2.0, 3.0]).unwrap(),
            "rect 0 0 2 3"
        );
        assert!(pts.is_empty());

        click(Tool::Arc, &mut pts, [0.0, 0.0]);
        click(Tool::Arc, &mut pts, [1.0, 1.0]);
        assert_eq!(
            click(Tool::Arc, &mut pts, [2.0, 0.0]).unwrap(),
            "arc-3p 0 0 1 1 2 0"
        );
        assert!(pts.is_empty());
        click(Tool::Arc, &mut pts, [0.0, 0.0]);
        click(Tool::Arc, &mut pts, [1.0, 1.0]);
        assert_eq!(
            click(Tool::Arc, &mut pts, [2.0, 2.0]),
            None,
            "a straight line is no arc"
        );
    }

    #[test]
    fn polylines_collect_until_finished() {
        let mut pts = Vec::new();
        for p in [[0.0, 0.0], [5.0, 0.0], [5.0, 0.0], [5.0, 5.0]] {
            assert_eq!(click(Tool::Polyline, &mut pts, p), None);
        }
        assert_eq!(pts.len(), 3, "a repeated point is dropped");
        assert_eq!(polyline(&pts, false).unwrap(), "pline 0 0 0 0 5 0 0 5 5 0");
        assert_eq!(polyline(&pts, true).unwrap(), "pline 1 0 0 0 5 0 0 5 5 0");
        assert_eq!(polyline(&pts[..1], false), None);
    }

    #[test]
    fn select_and_erase_take_no_points_and_bad_points_are_ignored() {
        let mut pts = Vec::new();
        assert_eq!(click(Tool::Select, &mut pts, [1.0, 1.0]), None);
        assert_eq!(click(Tool::Erase, &mut pts, [1.0, 1.0]), None);
        assert_eq!(click(Tool::Line, &mut pts, [f64::NAN, 1.0]), None);
        assert!(pts.is_empty());
        assert!(!Tool::Select.takes_points() && Tool::Line.takes_points());
    }

    #[test]
    fn words_start_tools_and_every_tool_has_a_prompt() {
        for t in Tool::ALL {
            assert_eq!(
                Tool::from_word(t.name()),
                Some(t)
                    .filter(|t| *t != Tool::Select)
                    .or(Some(Tool::Select))
            );
            for n in 0..4 {
                assert!(!t.prompt(n).is_empty());
            }
        }
        assert_eq!(Tool::from_word("L"), Some(Tool::Line));
        assert_eq!(Tool::from_word("pl"), Some(Tool::Polyline));
        assert_eq!(Tool::from_word("zoom"), None);
    }

    #[test]
    fn the_rubber_band_follows_the_pointer() {
        assert!(preview(Tool::Line, &[], [1.0, 1.0]).is_empty());
        assert_eq!(preview(Tool::Line, &[[0.0, 0.0]], [3.0, 4.0]).len(), 1);
        assert_eq!(preview(Tool::Circle, &[[0.0, 0.0]], [3.0, 4.0]).len(), 1);
        assert!(preview(Tool::Circle, &[[0.0, 0.0]], [0.0, 0.0]).is_empty());
        assert_eq!(preview(Tool::Rectangle, &[[0.0, 0.0]], [3.0, 4.0]).len(), 1);
        assert_eq!(
            preview(Tool::Polyline, &[[0.0, 0.0], [1.0, 1.0]], [3.0, 4.0]).len(),
            1
        );
        assert!(preview(Tool::Select, &[[0.0, 0.0]], [3.0, 4.0]).is_empty());
        assert!(preview(Tool::Line, &[[0.0, 0.0]], [f64::NAN, 4.0]).is_empty());
    }
}
