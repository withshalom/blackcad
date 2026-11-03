//! Triangulating even-odd fills.
//!
//! The display list gives a fill as rings under the even-odd rule: a ring
//! inside one ring is a hole, a ring inside the hole is an island again. This
//! sorts the rings by how deeply they nest, then cuts each outer ring (with its
//! holes) into triangles by ear clipping: the holes are joined to the outer
//! ring by bridges, and ears are cut until the ring is gone.
//!
//! The ear clipping follows the published algorithm of David Eberly,
//! "Triangulation by Ear Clipping" (Geometric Tools, 2002), with the hole
//! bridging and the repair passes (local intersections, splitting) that
//! robust implementations add. It is written for this project; no code is
//! copied. Work is bounded: a ring that cannot be cut within the budget
//! falls back to a fan, and the fill still draws.

type P = [f64; 2];

struct Node {
    /// Index of the point in the input (for the triangles), or `usize::MAX` for a copy.
    i: usize,
    x: f64,
    y: f64,
    prev: usize,
    next: usize,
    steiner: bool,
}

struct Poly {
    n: Vec<Node>,
    steps: usize,
    budget: usize,
}

/// Twice the signed area of a ring (positive counter-clockwise).
pub fn area2(ring: &[P]) -> f64 {
    let mut s = 0.0;
    for k in 0..ring.len() {
        let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
        s += a[0] * b[1] - b[0] * a[1];
    }
    s
}

fn point_in_ring(p: P, ring: &[P]) -> bool {
    let mut inside = false;
    let mut j = ring.len().wrapping_sub(1);
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Triangles (three points each) covering what the rings enclose under the even-odd rule.
/// Also says whether every ring was cut properly (`false`: a fallback was used somewhere).
pub fn triangulate(rings: &[Vec<P>]) -> (Vec<P>, bool) {
    // Clean: finite points, no closing repeat, no repeated neighbours, at least three points.
    let rings: Vec<Vec<P>> = rings
        .iter()
        .filter_map(|r| {
            let mut v: Vec<P> = Vec::with_capacity(r.len());
            for p in r.iter().filter(|p| p[0].is_finite() && p[1].is_finite()) {
                if v.last() != Some(p) {
                    v.push(*p);
                }
            }
            while v.len() > 1 && v.first() == v.last() {
                v.pop();
            }
            (v.len() >= 3 && area2(&v).abs() > 0.0).then_some(v)
        })
        .collect();
    let n = rings.len();
    let area: Vec<f64> = rings.iter().map(|r| area2(r).abs()).collect();
    // Nesting: which larger rings contain each ring (tested at its first point).
    let mut depth = vec![0usize; n];
    let mut parent: Vec<Option<usize>> = vec![None; n];
    for i in 0..n {
        let mut best: Option<usize> = None;
        for j in 0..n {
            if j != i && area[j] > area[i] && point_in_ring(rings[i][0], &rings[j]) {
                depth[i] += 1;
                if best.is_none_or(|b| area[j] < area[b]) {
                    best = Some(j);
                }
            }
        }
        parent[i] = best;
    }
    let mut out: Vec<P> = Vec::new();
    let mut exact = true;
    for i in 0..n {
        if !depth[i].is_multiple_of(2) {
            continue;
        }
        let holes: Vec<&Vec<P>> = (0..n).filter(|&j| depth[j] % 2 == 1 && parent[j] == Some(i)).map(|j| &rings[j]).collect();
        let (tris, ok) = earcut(&rings[i], &holes);
        exact &= ok;
        out.extend(tris);
    }
    (out, exact)
}

/// One outer ring with holes: its triangles, and whether the ear clipping finished.
fn earcut(outer: &[P], holes: &[&Vec<P>]) -> (Vec<P>, bool) {
    let mut pts: Vec<P> = outer.to_vec();
    let mut starts = Vec::new();
    for h in holes {
        starts.push(pts.len());
        pts.extend(h.iter().copied());
    }
    let mut poly = Poly { n: Vec::with_capacity(pts.len() + 8), steps: 0, budget: 3 * pts.len() * pts.len().min(2000) + 10_000 };
    let Some(mut ring) = poly.linked(&pts, 0, outer.len(), true) else { return (Vec::new(), true) };
    if poly.n[ring].next == poly.n[ring].prev {
        return (Vec::new(), true);
    }
    if !starts.is_empty() {
        ring = poly.eliminate_holes(&pts, &starts, outer.len(), ring);
    }
    let mut tris: Vec<usize> = Vec::new();
    poly.cut(Some(ring), &mut tris, 0);
    if poly.steps > poly.budget {
        // Out of budget: a fan from the first outer point. Right for convex rings, a bounded
        // approximation otherwise; it keeps the fill visible rather than missing.
        let mut fan = Vec::new();
        for k in 1..outer.len().saturating_sub(1) {
            fan.extend([outer[0], outer[k], outer[k + 1]]);
        }
        return (fan, false);
    }
    let covered: f64 = tris.chunks_exact(3).map(|t| area2(&[pts[t[0]], pts[t[1]], pts[t[2]]]).abs()).sum();
    let want = area2(outer).abs() - holes.iter().map(|h| area2(h).abs()).sum::<f64>();
    let ok = (covered - want).abs() <= want.abs() * 1e-6 + 1e-12;
    let out = tris.chunks_exact(3).flat_map(|t| [pts[t[0]], pts[t[1]], pts[t[2]]]).collect();
    (out, ok)
}

fn sign(v: f64) -> i8 {
    if v > 0.0 {
        1
    } else if v < 0.0 {
        -1
    } else {
        0
    }
}

impl Poly {
    fn spend(&mut self) -> bool {
        self.steps += 1;
        self.steps <= self.budget
    }

    fn insert(&mut self, i: usize, p: P, last: Option<usize>) -> usize {
        let id = self.n.len();
        match last {
            None => self.n.push(Node { i, x: p[0], y: p[1], prev: id, next: id, steiner: false }),
            Some(l) => {
                let nx = self.n[l].next;
                self.n.push(Node { i, x: p[0], y: p[1], prev: l, next: nx, steiner: false });
                self.n[nx].prev = id;
                self.n[l].next = id;
            }
        }
        id
    }

    fn remove(&mut self, p: usize) {
        let (pr, nx) = (self.n[p].prev, self.n[p].next);
        self.n[nx].prev = pr;
        self.n[pr].next = nx;
    }

    /// The points `pts[start..end]` as a ring wound counter-clockwise (outer) or clockwise (hole).
    fn linked(&mut self, pts: &[P], start: usize, end: usize, ccw: bool) -> Option<usize> {
        let seg = pts.get(start..end)?;
        let mut last: Option<usize> = None;
        if (area2(seg) > 0.0) == ccw {
            for (k, p) in seg.iter().enumerate() {
                last = Some(self.insert(start + k, *p, last));
            }
        } else {
            for (k, p) in seg.iter().enumerate().rev() {
                last = Some(self.insert(start + k, *p, last));
            }
        }
        let l = last?;
        if self.eq(l, self.n[l].next) {
            self.remove(l);
            return Some(self.n[l].next);
        }
        Some(l)
    }

    fn eq(&self, a: usize, b: usize) -> bool {
        self.n[a].x == self.n[b].x && self.n[a].y == self.n[b].y
    }

    /// Twice the signed area of the triangle p, q, r (negative: p, q, r turn counter-clockwise
    /// in a y-up system; this is the sign convention the tests below pin down).
    fn area(&self, p: usize, q: usize, r: usize) -> f64 {
        let (p, q, r) = (&self.n[p], &self.n[q], &self.n[r]);
        (q.y - p.y) * (r.x - q.x) - (q.x - p.x) * (r.y - q.y)
    }

    fn filter(&mut self, start: usize, end: Option<usize>) -> usize {
        let mut end = end.unwrap_or(start);
        let mut p = start;
        loop {
            let mut again = false;
            let (pr, nx) = (self.n[p].prev, self.n[p].next);
            if !self.n[p].steiner && (self.eq(p, nx) || self.area(pr, p, nx) == 0.0) {
                self.remove(p);
                p = pr;
                end = pr;
                if p == self.n[p].next {
                    break;
                }
                again = true;
            } else {
                p = nx;
            }
            if !(again || p != end) || !self.spend() {
                break;
            }
        }
        end
    }

    fn cut(&mut self, ear: Option<usize>, tris: &mut Vec<usize>, pass: u8) {
        let Some(mut ear) = ear else { return };
        let mut stop = ear;
        while self.n[ear].prev != self.n[ear].next {
            if !self.spend() {
                return;
            }
            let (prev, next) = (self.n[ear].prev, self.n[ear].next);
            if self.is_ear(ear) {
                tris.extend([self.n[prev].i, self.n[ear].i, self.n[next].i]);
                self.remove(ear);
                ear = self.n[next].next;
                stop = ear;
                continue;
            }
            ear = next;
            if ear == stop {
                match pass {
                    0 => {
                        let f = self.filter(ear, None);
                        self.cut(Some(f), tris, 1);
                    }
                    1 => {
                        let f = self.filter(ear, None);
                        let c = self.cure_local(f, tris);
                        self.cut(Some(c), tris, 2);
                    }
                    _ => self.split_cut(ear, tris),
                }
                return;
            }
        }
    }

    fn is_ear(&mut self, ear: usize) -> bool {
        let (a, b, c) = (self.n[ear].prev, ear, self.n[ear].next);
        if self.area(a, b, c) >= 0.0 {
            return false; // reflex or flat
        }
        let mut p = self.n[c].next;
        while p != a {
            if !self.spend() {
                return false;
            }
            let pn = &self.n[p];
            let (an, bn, cn) = (&self.n[a], &self.n[b], &self.n[c]);
            if !(an.x == pn.x && an.y == pn.y)
                && in_triangle([an.x, an.y], [bn.x, bn.y], [cn.x, cn.y], [pn.x, pn.y])
                && self.area(self.n[p].prev, p, self.n[p].next) >= 0.0
            {
                return false;
            }
            p = self.n[p].next;
        }
        true
    }

    fn cure_local(&mut self, start: usize, tris: &mut Vec<usize>) -> usize {
        let mut start = start;
        let mut p = start;
        loop {
            if !self.spend() {
                break;
            }
            let a = self.n[p].prev;
            let b = self.n[self.n[p].next].next;
            let pn = self.n[p].next;
            if !self.eq(a, b) && self.intersects(a, p, pn, b) && self.locally_inside(a, b) && self.locally_inside(b, a) {
                tris.extend([self.n[a].i, self.n[p].i, self.n[b].i]);
                self.remove(p);
                self.remove(pn);
                start = b;
                p = b;
            }
            p = self.n[p].next;
            if p == start {
                break;
            }
        }
        self.filter(p, None)
    }

    fn split_cut(&mut self, start: usize, tris: &mut Vec<usize>) {
        let mut a = start;
        loop {
            let mut b = self.n[self.n[a].next].next;
            while b != self.n[a].prev {
                if !self.spend() {
                    return;
                }
                if self.n[a].i != self.n[b].i && self.valid_diagonal(a, b) {
                    let mut c = self.split(a, b);
                    let an = self.n[a].next;
                    a = self.filter(a, Some(an));
                    let cn = self.n[c].next;
                    c = self.filter(c, Some(cn));
                    self.cut(Some(a), tris, 0);
                    self.cut(Some(c), tris, 0);
                    return;
                }
                b = self.n[b].next;
            }
            a = self.n[a].next;
            if a == start {
                return;
            }
        }
    }

    fn eliminate_holes(&mut self, pts: &[P], starts: &[usize], outer_len: usize, outer: usize) -> usize {
        let mut queue: Vec<usize> = Vec::new();
        for (k, s) in starts.iter().enumerate() {
            let end = starts.get(k + 1).copied().unwrap_or(pts.len());
            let _ = outer_len;
            if let Some(list) = self.linked(pts, *s, end, false) {
                if list == self.n[list].next {
                    self.n[list].steiner = true;
                }
                queue.push(self.leftmost(list));
            }
        }
        queue.sort_by(|a, b| self.n[*a].x.partial_cmp(&self.n[*b].x).unwrap_or(std::cmp::Ordering::Equal));
        let mut outer = outer;
        for hole in queue {
            outer = self.eliminate_hole(hole, outer);
        }
        outer
    }

    fn eliminate_hole(&mut self, hole: usize, outer: usize) -> usize {
        let Some(bridge) = self.hole_bridge(hole, outer) else { return outer };
        let rev = self.split(bridge, hole);
        let rn = self.n[rev].next;
        self.filter(rev, Some(rn));
        let bn = self.n[bridge].next;
        self.filter(bridge, Some(bn))
    }

    fn hole_bridge(&mut self, hole: usize, outer: usize) -> Option<usize> {
        let (hx, hy) = (self.n[hole].x, self.n[hole].y);
        let mut p = outer;
        let mut qx = f64::NEG_INFINITY;
        let mut m: Option<usize> = None;
        loop {
            if !self.spend() {
                return None;
            }
            let nx = self.n[p].next;
            let (px, py, nxx, nyy) = (self.n[p].x, self.n[p].y, self.n[nx].x, self.n[nx].y);
            if hy <= py && hy >= nyy && nyy != py {
                let x = px + (hy - py) * (nxx - px) / (nyy - py);
                if x <= hx && x > qx {
                    qx = x;
                    m = Some(if px < nxx { p } else { nx });
                    if x == hx {
                        return m;
                    }
                }
            }
            p = nx;
            if p == outer {
                break;
            }
        }
        let mut m = m?;
        let stop = m;
        let (mx, my) = (self.n[m].x, self.n[m].y);
        let mut tan_min = f64::INFINITY;
        p = m;
        loop {
            if !self.spend() {
                return None;
            }
            let (px, py) = (self.n[p].x, self.n[p].y);
            if hx >= px && px >= mx && hx != px {
                let (t0, t2) = if hy < my { (hx, qx) } else { (qx, hx) };
                if in_triangle([t0, hy], [mx, my], [t2, hy], [px, py]) {
                    let tan = (hy - py).abs() / (hx - px);
                    if self.locally_inside(p, hole)
                        && (tan < tan_min || (tan == tan_min && (px > self.n[m].x || (px == self.n[m].x && self.sector_contains(m, p)))))
                    {
                        m = p;
                        tan_min = tan;
                    }
                }
            }
            p = self.n[p].next;
            if p == stop {
                break;
            }
        }
        Some(m)
    }

    fn sector_contains(&self, m: usize, p: usize) -> bool {
        self.area(self.n[m].prev, m, self.n[p].prev) < 0.0 && self.area(self.n[p].next, m, self.n[m].next) < 0.0
    }

    fn leftmost(&self, start: usize) -> usize {
        let mut p = start;
        let mut l = start;
        loop {
            let (a, b) = (&self.n[p], &self.n[l]);
            if a.x < b.x || (a.x == b.x && a.y < b.y) {
                l = p;
            }
            p = a.next;
            if p == start {
                return l;
            }
        }
    }

    fn valid_diagonal(&mut self, a: usize, b: usize) -> bool {
        let (an, ap, bn, bp) = (self.n[a].next, self.n[a].prev, self.n[b].next, self.n[b].prev);
        self.n[an].i != self.n[b].i
            && self.n[ap].i != self.n[b].i
            && !self.intersects_poly(a, b)
            && ((self.locally_inside(a, b)
                && self.locally_inside(b, a)
                && self.middle_inside(a, b)
                && (self.area(ap, a, bp) != 0.0 || self.area(a, bp, b) != 0.0))
                || (self.eq(a, b) && self.area(ap, a, an) > 0.0 && self.area(bp, b, bn) > 0.0))
    }

    fn intersects(&self, p1: usize, q1: usize, p2: usize, q2: usize) -> bool {
        let o1 = sign(self.area(p1, q1, p2));
        let o2 = sign(self.area(p1, q1, q2));
        let o3 = sign(self.area(p2, q2, p1));
        let o4 = sign(self.area(p2, q2, q1));
        (o1 != o2 && o3 != o4)
            || (o1 == 0 && self.on_segment(p1, p2, q1))
            || (o2 == 0 && self.on_segment(p1, q2, q1))
            || (o3 == 0 && self.on_segment(p2, p1, q2))
            || (o4 == 0 && self.on_segment(p2, q1, q2))
    }

    fn on_segment(&self, p: usize, q: usize, r: usize) -> bool {
        let (p, q, r) = (&self.n[p], &self.n[q], &self.n[r]);
        q.x <= p.x.max(r.x) && q.x >= p.x.min(r.x) && q.y <= p.y.max(r.y) && q.y >= p.y.min(r.y)
    }

    fn intersects_poly(&mut self, a: usize, b: usize) -> bool {
        let mut p = a;
        loop {
            if !self.spend() {
                return true;
            }
            let pn = self.n[p].next;
            if self.n[p].i != self.n[a].i && self.n[pn].i != self.n[a].i && self.n[p].i != self.n[b].i && self.n[pn].i != self.n[b].i && self.intersects(p, pn, a, b) {
                return true;
            }
            p = pn;
            if p == a {
                return false;
            }
        }
    }

    fn locally_inside(&self, a: usize, b: usize) -> bool {
        let (ap, an) = (self.n[a].prev, self.n[a].next);
        if self.area(ap, a, an) < 0.0 {
            self.area(a, b, an) >= 0.0 && self.area(a, ap, b) >= 0.0
        } else {
            self.area(a, b, ap) < 0.0 || self.area(a, an, b) < 0.0
        }
    }

    fn middle_inside(&self, a: usize, b: usize) -> bool {
        let mut p = a;
        let mut inside = false;
        let (px, py) = ((self.n[a].x + self.n[b].x) / 2.0, (self.n[a].y + self.n[b].y) / 2.0);
        loop {
            let nx = self.n[p].next;
            let (c, d) = (&self.n[p], &self.n[nx]);
            if (c.y > py) != (d.y > py) && d.y != c.y && px < (d.x - c.x) * (py - c.y) / (d.y - c.y) + c.x {
                inside = !inside;
            }
            p = nx;
            if p == a {
                return inside;
            }
        }
    }

    /// Join `a` and `b` by a diagonal: the ring splits in two; the new node (a copy of `b`) is returned.
    fn split(&mut self, a: usize, b: usize) -> usize {
        let a2 = self.n.len();
        let b2 = a2 + 1;
        let (an, bp) = (self.n[a].next, self.n[b].prev);
        let (ai, ax, ay) = (self.n[a].i, self.n[a].x, self.n[a].y);
        let (bi, bx, by) = (self.n[b].i, self.n[b].x, self.n[b].y);
        self.n.push(Node { i: ai, x: ax, y: ay, prev: b2, next: an, steiner: false });
        self.n.push(Node { i: bi, x: bx, y: by, prev: bp, next: a2, steiner: false });
        self.n[a].next = b;
        self.n[b].prev = a;
        self.n[an].prev = a2;
        self.n[bp].next = b2;
        b2
    }
}

fn in_triangle(a: P, b: P, c: P, p: P) -> bool {
    (c[0] - p[0]) * (a[1] - p[1]) >= (a[0] - p[0]) * (c[1] - p[1])
        && (a[0] - p[0]) * (b[1] - p[1]) >= (b[0] - p[0]) * (a[1] - p[1])
        && (b[0] - p[0]) * (c[1] - p[1]) >= (c[0] - p[0]) * (b[1] - p[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri_area(t: &[P]) -> f64 {
        t.chunks_exact(3).map(|c| area2(c).abs() / 2.0).sum()
    }

    fn square(x: f64, y: f64, s: f64) -> Vec<P> {
        vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]]
    }

    #[test]
    fn a_square_is_two_triangles() {
        let (t, ok) = triangulate(&[square(0.0, 0.0, 2.0)]);
        assert!(ok);
        assert_eq!(t.len(), 6);
        assert!((tri_area(&t) - 4.0).abs() < 1e-12);
    }

    #[test]
    fn either_winding_and_a_repeated_closing_point() {
        let mut r = square(0.0, 0.0, 3.0);
        r.reverse();
        r.push(r[0]);
        let (t, ok) = triangulate(&[r]);
        assert!(ok && (tri_area(&t) - 9.0).abs() < 1e-12);
    }

    #[test]
    fn a_concave_ring() {
        // An L: 3x3 with the top-right 2x2 removed = area 5.
        let l = vec![[0.0, 0.0], [3.0, 0.0], [3.0, 1.0], [1.0, 1.0], [1.0, 3.0], [0.0, 3.0]];
        let (t, ok) = triangulate(&[l]);
        assert!(ok);
        assert!((tri_area(&t) - 5.0).abs() < 1e-12, "{}", tri_area(&t));
    }

    #[test]
    fn a_hole_is_left_open() {
        let (t, ok) = triangulate(&[square(0.0, 0.0, 10.0), square(3.0, 3.0, 4.0)]);
        assert!(ok);
        assert!((tri_area(&t) - 84.0).abs() < 1e-9, "{}", tri_area(&t));
    }

    #[test]
    fn nesting_alternates_and_separate_rings_both_fill() {
        // 10 square, 6 hole, 2 island inside the hole, and a separate 1 square beside.
        let rings = vec![square(0.0, 0.0, 10.0), square(2.0, 2.0, 6.0), square(4.0, 4.0, 2.0), square(20.0, 0.0, 1.0)];
        let (t, ok) = triangulate(&rings);
        assert!(ok);
        assert!((tri_area(&t) - (100.0 - 36.0 + 4.0 + 1.0)).abs() < 1e-9, "{}", tri_area(&t));
    }

    #[test]
    fn two_holes() {
        let rings = vec![square(0.0, 0.0, 20.0), square(2.0, 2.0, 4.0), square(10.0, 10.0, 5.0)];
        let (t, ok) = triangulate(&rings);
        assert!(ok);
        assert!((tri_area(&t) - (400.0 - 16.0 - 25.0)).abs() < 1e-9);
    }

    #[test]
    fn a_circle_of_many_points() {
        let n = 720;
        let c: Vec<P> = (0..n).map(|k| {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            [5.0 * a.cos(), 5.0 * a.sin()]
        }).collect();
        let hole: Vec<P> = (0..n).map(|k| {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            [2.0 * a.cos(), 2.0 * a.sin()]
        }).collect();
        let (t, ok) = triangulate(&[c.clone(), hole.clone()]);
        assert!(ok);
        let want = area2(&c).abs() / 2.0 - area2(&hole).abs() / 2.0;
        assert!((tri_area(&t) - want).abs() < 1e-6);
    }

    #[test]
    fn degenerate_and_hostile_rings_do_not_panic() {
        let bad: Vec<Vec<Vec<P>>> = vec![
            vec![],
            vec![vec![]],
            vec![vec![[0.0, 0.0], [1.0, 1.0]]],
            vec![vec![[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]]], // collinear
            vec![vec![[0.0, 0.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0]]],
            vec![vec![[f64::NAN, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]],
            vec![vec![[f64::INFINITY, 0.0], [1.0, 0.0], [1.0, 1.0]]],
            vec![vec![[1e300, 1e300], [-1e300, 1e300], [0.0, -1e300]]],
            vec![vec![[0.0, 0.0], [4.0, 4.0], [4.0, 0.0], [0.0, 4.0]]], // bow tie
        ];
        for rings in bad {
            let _ = triangulate(&rings);
        }
    }

    /// A deterministic generator for the fuzz tests.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
        }
    }

    #[test]
    fn random_star_polygons_cover_their_area() {
        let mut r = Rng(7);
        for case in 0..300 {
            let n = 3 + (r.next() * 60.0) as usize;
            let cx = r.next() * 100.0;
            let cy = r.next() * 100.0;
            // Radii vary at random but angles increase, so the polygon is simple.
            let ring: Vec<P> = (0..n).map(|k| {
                let a = std::f64::consts::TAU * (k as f64 + r.next() * 0.9) / n as f64;
                let rad = 1.0 + r.next() * 9.0;
                [cx + rad * a.cos(), cy + rad * a.sin()]
            }).collect();
            let want = area2(&ring).abs() / 2.0;
            let (t, ok) = triangulate(&[ring]);
            assert!(ok, "case {case} (n={n}) fell back");
            assert!((tri_area(&t) - want).abs() <= want * 1e-9 + 1e-9, "case {case}: {} vs {want}", tri_area(&t));
        }
    }

    #[test]
    fn random_garbage_never_panics_and_stays_bounded() {
        let mut r = Rng(99);
        for _ in 0..200 {
            let n = (r.next() * 40.0) as usize;
            let ring: Vec<P> = (0..n).map(|_| [r.next() * 10.0, r.next() * 10.0]).collect();
            let ring2: Vec<P> = (0..n / 2).map(|_| [r.next() * 10.0, r.next() * 10.0]).collect();
            let (t, _) = triangulate(&[ring, ring2]);
            assert!(t.len() % 3 == 0);
        }
    }

    #[test]
    fn a_big_ring_stays_within_its_budget() {
        // 20 000 points on a spiral-ish wobble: must return, fast, with something drawable.
        let n = 20_000;
        let ring: Vec<P> = (0..n).map(|k| {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            let rad = 100.0 + 5.0 * (a * 37.0).sin();
            [rad * a.cos(), rad * a.sin()]
        }).collect();
        let t0 = std::time::Instant::now();
        let (t, _) = triangulate(&[ring]);
        assert!(!t.is_empty());
        assert!(t0.elapsed().as_secs() < 10, "{:?}", t0.elapsed());
    }
}
