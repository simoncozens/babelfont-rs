use std::collections::HashSet;

use kurbo::{common::GAUSS_LEGENDRE_COEFFS_4, CubicBez, ParamCurve, ParamCurveArclen, Vec2};

use crate::{BabelfontError, Node, NodeType};

const fn bernstein(t: f64) -> (f64, f64, f64, f64) {
    let mt = 1.0 - t;
    (mt * mt * mt, 3.0 * t * mt * mt, 3.0 * t * t * mt, t * t * t)
}

const fn compute_m() -> (f64, f64, f64) {
    let mut m11 = 0.0;
    let mut m22 = 0.0;
    let mut m12 = 0.0;
    let mut i = 0;
    while i < 4 {
        let (weight, x) = GAUSS_LEGENDRE_COEFFS_4[i];
        // Domain of GAUSS_LEGENDRE_COEFFS_4 is [-1, 1], but we want [0, 1], so we remap.
        let t = 0.5 * (x + 1.0);
        let w = 0.5 * weight;
        let (_, b1, b2, _) = bernstein(t);
        m11 += w * b1 * b1;
        m22 += w * b2 * b2;
        m12 += w * b1 * b2;
        i += 1;
    }
    (m11, m22, m12)
}

const M: (f64, f64, f64) = compute_m();
const M11: f64 = M.0;
const M22: f64 = M.1;
const M12: f64 = M.2;

fn eval_ab(a: &CubicBez, b: &CubicBez, r: f64, t: f64) -> kurbo::Point {
    if t < r {
        a.eval(t / r)
    } else {
        b.eval((t - r) / (1.0 - r))
    }
}

pub fn join_bez(a: &CubicBez, b: &CubicBez, tolerance: f64) -> Option<CubicBez> {
    let d1 = a.p1 - a.p0;
    let d2 = b.p3 - b.p2;
    let len_a = a.arclen(0.1);
    let len_b = b.arclen(0.1);
    let r = (len_a / (len_a + len_b)).clamp(1.0e-3, 1.0 - 1.0e-3);
    let mut r1 = Vec2::ZERO;
    let mut r2 = Vec2::ZERO;
    for (weight, x) in GAUSS_LEGENDRE_COEFFS_4 {
        let t = 0.5 * (x + 1.0);
        let w = 0.5 * weight;
        let bt = bernstein(t);

        let fixed = (bt.0 + bt.1) * a.p0.to_vec2() + (bt.2 + bt.3) * b.p3.to_vec2();
        let sample = eval_ab(a, b, r, t).to_vec2();
        let resid = sample - fixed;
        r1 += w * bt.1 * resid;
        r2 += w * bt.2 * resid;
    }
    let d1_dot_d1 = d1.dot(d1);
    let d2_dot_d2 = d2.dot(d2);
    let d1_dot_d2 = d1.dot(d2);

    let mat11 = d1_dot_d1 * M11;
    let mat22 = d2_dot_d2 * M22;
    let mat12 = -d1_dot_d2 * M12;

    let rhs1 = d1.dot(r1);
    let rhs2 = -d2.dot(r2);

    // Solve matrix by Cramer's rule. The matrix is symmetric, so we can just compute the determinant once.
    let det = mat11 * mat22 - mat12 * mat12;
    let (t1, t2) = if det.abs() > 1e-12 {
        (
            (rhs1 * mat22 - mat12 * rhs2) / det,
            (mat11 * rhs2 - mat12 * rhs1) / det,
        )
    } else {
        // Degenerate (near-parallel or zero-length tangent handles);
        // fall back to something safe rather than dividing by ~0.
        (0.0, 0.0)
    };

    let c1 = a.p0 + t1 * d1;
    let c2 = b.p3 - t2 * d2;
    let fitted = CubicBez::new(a.p0, c1, c2, b.p3);

    // Error check
    let n = 64;
    let mut max_err = 0.0f64;
    for i in 0..=n {
        let t = i as f64 / n as f64;
        let e = (eval_ab(a, b, r, t) - fitted.eval(t)).hypot();
        max_err = max_err.max(e);
    }
    if max_err < tolerance {
        Some(fitted)
    } else {
        None
    }
}

/// Delete the nodes at the given indices from a path, keeping its shape where
/// possible.
///
/// The path is treated as a list of segments, each running from one on-curve
/// node to the next (wrapping around for closed paths). The deleted nodes are
/// skipped over, and the segments they belonged to are replaced as follows:
///
/// * If a deleted node is an off-curve of a segment, the segment is reduced to
///   a line between its two on-curve nodes.
/// * If a single curve node is deleted between two curve nodes whose off-curves
///   are untouched, [`join_bez`] is used to fit one curve across the deleted
///   node; if the fit is not within `tolerance`, a line is used instead.
/// * Anything else — several consecutive curve nodes, line nodes, quadratic
///   segments — collapses to a line between the surviving end nodes.
///
/// Returns the new node list, with `closed` unchanged. Deleting every on-curve
/// node produces an empty node list; deleting the Move node of an open path
/// makes the path start at the next surviving node. Out-of-range indices are
/// ignored, and a malformed input path returns [`BabelfontError::BadPath`].
pub fn delete_keeping_shape(
    nodes: &[Node],
    closed: bool,
    to_delete: &[usize],
    tolerance: f64,
) -> Result<Vec<Node>, BabelfontError> {
    if nodes.is_empty() || to_delete.is_empty() {
        return Ok(nodes.to_vec());
    }
    Ok(Deleter::new(nodes, closed, to_delete, tolerance)?.delete())
}

/// A segment of a path between two consecutive on-curve nodes, expressed as
/// indices into the path's node list.
struct Segment {
    /// Node index of the segment's start (an on-curve node).
    start: usize,
    /// Node index of the segment's end (an on-curve node).
    end: usize,
    /// Node indices of the off-curve nodes of the segment.
    offs: Vec<usize>,
    /// Set when one of the segment's off-curves is deleted, forcing the
    /// segment to be reduced to a line.
    reduce_to_line: bool,
}

/// A segment of the rebuilt path: its off-curve nodes and the on-curve node it
/// ends at.
struct MergedSegment {
    offs: Vec<Node>,
    end: Node,
}

/// State for [`delete_keeping_shape`]: the original path, the deletion list
/// and the segments derived from it.
struct Deleter<'a> {
    nodes: &'a [Node],
    closed: bool,
    on_curve: Vec<usize>,
    segments: Vec<Segment>,
    to_delete: HashSet<usize>,
    tolerance: f64,
}

impl<'a> Deleter<'a> {
    fn new(
        nodes: &'a [Node],
        closed: bool,
        to_delete: &[usize],
        tolerance: f64,
    ) -> Result<Self, BabelfontError> {
        let to_delete: HashSet<usize> = to_delete
            .iter()
            .copied()
            .filter(|ix| *ix < nodes.len())
            .collect();
        let on_curve: Vec<usize> = nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.nodetype != NodeType::OffCurve)
            .map(|(ix, _)| ix)
            .collect();
        if on_curve.is_empty() {
            return Err(BabelfontError::BadPath);
        }
        // An open path's off-curves must all sit between on-curve nodes;
        // anywhere else and we could not rebuild the node list faithfully.
        if !closed && (on_curve[0] != 0 || on_curve[on_curve.len() - 1] != nodes.len() - 1) {
            return Err(BabelfontError::BadPath);
        }
        let mut segments = build_segments(nodes, closed, &on_curve);
        for seg in &mut segments {
            seg.reduce_to_line = seg.offs.iter().any(|ix| to_delete.contains(ix));
        }
        Ok(Deleter {
            nodes,
            closed,
            on_curve,
            segments,
            to_delete,
            tolerance,
        })
    }

    fn delete(self) -> Vec<Node> {
        if self.to_delete.is_empty() {
            return self.nodes.to_vec();
        }
        if self.closed {
            self.delete_closed()
        } else {
            self.delete_open()
        }
    }

    fn delete_closed(&self) -> Vec<Node> {
        let m = self.on_curve.len();
        let Some(first_pos) = self
            .on_curve
            .iter()
            .position(|ix| !self.to_delete.contains(ix))
        else {
            return Vec::new();
        };
        let mut result = Vec::new();
        let mut run_start = first_pos;
        // Walk the on-curve nodes cyclically; every surviving node closes the
        // run of segments which started at the previous survivor.
        for step in 1..=m {
            let pos = (first_pos + step) % m;
            if self.to_delete.contains(&self.on_curve[pos]) {
                continue;
            }
            let k = (pos + m - run_start) % m;
            result.push(self.merge_run(run_start, if k == 0 { m } else { k }));
            run_start = pos;
        }
        // The walk produces the runs between consecutive survivors first; the
        // wrapping run (from the last survivor back to the first) must lead the
        // node list so the first on-curve node keeps its position.
        result.rotate_right(1);

        let mut out = Vec::new();
        for seg in result {
            out.extend(seg.offs);
            out.push(seg.end);
        }
        out
    }

    fn delete_open(&self) -> Vec<Node> {
        let survivors: Vec<usize> = self
            .on_curve
            .iter()
            .enumerate()
            .filter(|(_, &ix)| !self.to_delete.contains(&ix))
            .map(|(pos, _)| pos)
            .collect();
        if survivors.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        // If the original Move node was deleted, the path now starts at the
        // first surviving node.
        let mut start = self.nodes[self.on_curve[survivors[0]]].clone();
        if survivors[0] > 0 {
            start.nodetype = NodeType::Move;
        }
        out.push(start);
        for pair in survivors.windows(2) {
            let seg = self.merge_run(pair[0], pair[1] - pair[0]);
            out.extend(seg.offs);
            out.push(seg.end);
        }
        out
    }

    /// Merge the `k` original segments starting at segment `a` into one.
    fn merge_run(&self, a: usize, k: usize) -> MergedSegment {
        let end_ix = self.on_curve[(a + k) % self.on_curve.len()];
        if k == 1 {
            let seg = &self.segments[a];
            if seg.reduce_to_line {
                return self.line_segment(end_ix);
            }
            return MergedSegment {
                offs: seg.offs.iter().map(|&ix| self.nodes[ix].clone()).collect(),
                end: self.nodes[end_ix].clone(),
            };
        }
        if k == 2 {
            if let Some(joined) = self.try_join(a) {
                return joined;
            }
        }
        self.line_segment(end_ix)
    }

    /// If a single curve node was deleted between two untouched cubic
    /// segments, fit one curve across it; `None` if the fit is not within
    /// tolerance or the segments are not cubic.
    fn try_join(&self, a: usize) -> Option<MergedSegment> {
        let m = self.on_curve.len();
        let seg1 = &self.segments[a];
        let seg2 = &self.segments[(a + 1) % m];
        let start_ix = self.on_curve[a];
        let mid_ix = self.on_curve[(a + 1) % m];
        let end_ix = self.on_curve[(a + 2) % m];
        if seg1.reduce_to_line
            || seg2.reduce_to_line
            || self.nodes[start_ix].nodetype != NodeType::Curve
            || self.nodes[mid_ix].nodetype != NodeType::Curve
            || self.nodes[end_ix].nodetype != NodeType::Curve
            || seg1.offs.len() != 2
            || seg2.offs.len() != 2
        {
            return None;
        }
        let fitted = join_bez(
            &self.cubic_from_segment(seg1),
            &self.cubic_from_segment(seg2),
            self.tolerance,
        )?;
        let mut end = self.nodes[end_ix].clone();
        end.nodetype = NodeType::Curve;
        Some(MergedSegment {
            offs: vec![
                Node::new_offcurve(fitted.p1.x, fitted.p1.y),
                Node::new_offcurve(fitted.p2.x, fitted.p2.y),
            ],
            end,
        })
    }

    fn line_segment(&self, end_ix: usize) -> MergedSegment {
        let mut end = self.nodes[end_ix].clone();
        end.nodetype = NodeType::Line;
        MergedSegment {
            offs: Vec::new(),
            end,
        }
    }

    /// The kurbo cubic for a segment with two off-curves.
    fn cubic_from_segment(&self, seg: &Segment) -> CubicBez {
        CubicBez::new(
            self.nodes[seg.start].to_kurbo(),
            self.nodes[seg.offs[0]].to_kurbo(),
            self.nodes[seg.offs[1]].to_kurbo(),
            self.nodes[seg.end].to_kurbo(),
        )
    }
}

/// Split the node list into the segments between consecutive on-curve nodes.
/// For closed paths the last segment wraps around to the first on-curve node
/// and picks up the leading off-curves.
fn build_segments(nodes: &[Node], closed: bool, on_curve: &[usize]) -> Vec<Segment> {
    let mut segments = Vec::with_capacity(on_curve.len());
    if closed {
        for i in 0..on_curve.len() {
            let end = on_curve[(i + 1) % on_curve.len()];
            segments.push(Segment {
                start: on_curve[i],
                end,
                offs: offcurves_between(nodes, on_curve[i], end),
                reduce_to_line: false,
            });
        }
    } else {
        for i in 0..on_curve.len().saturating_sub(1) {
            segments.push(Segment {
                start: on_curve[i],
                end: on_curve[i + 1],
                offs: offcurves_between(nodes, on_curve[i], on_curve[i + 1]),
                reduce_to_line: false,
            });
        }
    }
    segments
}

/// The indices of the off-curve nodes between two consecutive on-curve nodes,
/// walking forwards and wrapping around the end of the list.
fn offcurves_between(nodes: &[Node], from: usize, to: usize) -> Vec<usize> {
    let mut offs = Vec::new();
    let mut i = from + 1;
    while i != to {
        if i >= nodes.len() {
            i = 0;
            if i == to {
                break;
            }
        }
        offs.push(i);
        i += 1;
    }
    offs
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use kurbo::{ParamCurve, ParamCurveArclen, PathEl};

    use super::*;

    fn path_of(nodes: Vec<Node>, closed: bool) -> crate::Path {
        crate::Path {
            nodes,
            closed,
            ..Default::default()
        }
    }

    /// A circle of radius 100 made of four cubic quarter-arcs, in the
    /// preferred representation: closed, starting with the off-curves of the
    /// closing segment. The on-curve nodes C0..C3 sit at 0°, 90°, 180°, 270°.
    fn quarter_circle() -> Vec<Node> {
        let k = 4.0 / 3.0 * (std::f64::consts::PI / 8.0).tan() * 100.0;
        vec![
            Node::new_offcurve(k, -100.0), // closing arc C3 -> C0
            Node::new_offcurve(100.0, k),
            Node::new_curve(100.0, 0.0),  // C0
            Node::new_offcurve(100.0, k), // arc C0 -> C1
            Node::new_offcurve(-k, 100.0),
            Node::new_curve(0.0, 100.0),   // C1
            Node::new_offcurve(-k, 100.0), // arc C1 -> C2
            Node::new_offcurve(-100.0, -k),
            Node::new_curve(-100.0, 0.0),   // C2
            Node::new_offcurve(-100.0, -k), // arc C2 -> C3
            Node::new_offcurve(k, -100.0),
            Node::new_curve(0.0, -100.0), // C3
        ]
    }

    /// Sample `n + 1` points at equal arc-length intervals along a path.
    fn sample_at_arclen_fractions(path: &crate::Path, n: usize) -> Vec<kurbo::Point> {
        let bez = path.to_kurbo().unwrap();
        let mut segments: Vec<kurbo::PathSeg> = Vec::new();
        let mut start = kurbo::Point::ZERO;
        for el in bez.elements() {
            match el {
                PathEl::MoveTo(p) => start = *p,
                PathEl::LineTo(p) => {
                    segments.push(kurbo::PathSeg::Line(kurbo::Line::new(start, *p)));
                    start = *p;
                }
                PathEl::QuadTo(p1, p2) => {
                    segments.push(kurbo::PathSeg::Quad(kurbo::QuadBez::new(start, *p1, *p2)));
                    start = *p2;
                }
                PathEl::CurveTo(p1, p2, p3) => {
                    segments.push(kurbo::PathSeg::Cubic(kurbo::CubicBez::new(
                        start, *p1, *p2, *p3,
                    )));
                    start = *p3;
                }
                PathEl::ClosePath => {
                    segments.push(kurbo::PathSeg::Line(kurbo::Line::new(start, start)));
                }
            }
        }
        let total: f64 = segments.iter().map(|s| s.arclen(0.1)).sum();
        let mut result = Vec::with_capacity(n + 1);
        let mut seg_idx = 0;
        let mut acc = 0.0;
        for i in 0..=n {
            let target = total * i as f64 / n as f64;
            while seg_idx + 1 < segments.len() && acc + segments[seg_idx].arclen(0.1) <= target {
                acc += segments[seg_idx].arclen(0.1);
                seg_idx += 1;
            }
            let seg = segments[seg_idx];
            let seg_len = seg.arclen(0.1);
            let t = if seg_len > 1e-9 {
                seg.inv_arclen((target - acc).clamp(0.0, seg_len), 0.01)
            } else {
                0.0
            };
            result.push(seg.eval(t));
        }
        result
    }

    fn max_distance(a: &[kurbo::Point], b: &[kurbo::Point]) -> f64 {
        a.iter()
            .zip(b)
            .map(|(p, q)| (*p - *q).hypot())
            .fold(0.0, f64::max)
    }

    #[test]
    fn deleting_an_offcurve_reduces_its_segment_to_a_line() {
        let nodes = quarter_circle();
        // Delete the first off-curve of the arc C0 -> C1; that arc becomes a
        // line and its two off-curves are dropped, the others stay curves.
        let result = delete_keeping_shape(&nodes, true, &[3], 1.0).unwrap();
        assert_eq!(result.len(), 10);
        assert_eq!(result[3].nodetype, NodeType::Line);
        assert_eq!((result[3].x, result[3].y), (0.0, 100.0)); // C1
        assert_eq!(
            result
                .iter()
                .filter(|n| n.nodetype == NodeType::OffCurve)
                .count(),
            6
        );
        path_of(result, true).to_kurbo().unwrap();
    }

    /// A closed loop made of two cubic segments: from M = (0, 75) to
    /// P = (0, 0), and the closing segment back from P to M. The two are
    /// exactly the two halves of the parent loop cubic through
    /// (100, 100) and (-100, 100).
    fn cubic_loop() -> Vec<Node> {
        vec![
            Node::new_offcurve(50.0, 50.0), // closing segment (0, 0) -> (0, 75)
            Node::new_offcurve(25.0, 75.0),
            Node::new_curve(0.0, 75.0),      // M
            Node::new_offcurve(-25.0, 75.0), // segment (0, 75) -> (0, 0)
            Node::new_offcurve(-50.0, 50.0),
            Node::new_curve(0.0, 0.0), // P
        ]
    }

    #[test]
    fn deleting_a_curve_node_between_two_curves_keeps_the_shape() {
        let nodes = cubic_loop();
        // Deleting M joins the two halves back into the parent loop cubic,
        // whose control points are (100, 100) and (-100, 100).
        let result = delete_keeping_shape(&nodes, true, &[2], 1.0).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].nodetype, NodeType::OffCurve);
        assert_eq!(result[1].nodetype, NodeType::OffCurve);
        assert_eq!(result[2].nodetype, NodeType::Curve); // P
        assert_eq!((result[2].x, result[2].y), (0.0, 0.0));
        assert!((result[0].x - 100.0).abs() < 1.0 && (result[0].y - 100.0).abs() < 1.0);
        assert!((result[1].x + 100.0).abs() < 1.0 && (result[1].y - 100.0).abs() < 1.0);
        // The joined loop is the same shape as the two halves.
        let before = sample_at_arclen_fractions(&path_of(nodes, true), 256);
        let after = sample_at_arclen_fractions(&path_of(result, true), 256);
        assert!(max_distance(&before, &after) < 1.0);
    }

    #[test]
    fn a_curve_fit_outside_tolerance_collapses_to_a_line() {
        let nodes = quarter_circle();
        // The two quarter arcs on either side of C1 form a semicircle, which a
        // single cubic cannot approximate; with a tolerance below the fitting
        // error the run collapses to a line, while the untouched arcs keep
        // their shape.
        let result = delete_keeping_shape(&nodes, true, &[5], 1e-9).unwrap();
        assert_eq!(result.len(), 7);
        assert_eq!(result[2].nodetype, NodeType::Curve); // C0
        assert_eq!(result[3].nodetype, NodeType::Line); // C2
        assert_eq!((result[3].x, result[3].y), (-100.0, 0.0));
        assert_eq!(result[6].nodetype, NodeType::Curve); // C3
        path_of(result, true).to_kurbo().unwrap();
    }

    #[test]
    fn deleting_consecutive_curve_nodes_collapses_to_a_line() {
        let nodes = quarter_circle();
        // Deleting C1 and C2 collapses the three arcs between C0 and C3 to a
        // line; the closing arc C3 -> C0 keeps its shape.
        let result = delete_keeping_shape(&nodes, true, &[5, 8], 1.0).unwrap();
        assert_eq!(result.len(), 4);
        assert_eq!(result[2].nodetype, NodeType::Curve); // C0
        assert_eq!(result[3].nodetype, NodeType::Line); // C3
        assert_eq!((result[3].x, result[3].y), (0.0, -100.0));
        path_of(result, true).to_kurbo().unwrap();
    }

    #[test]
    fn deleting_an_offcurve_and_its_curve_node_gives_a_line() {
        let nodes = quarter_circle();
        // Delete C1 and the second off-curve of the arc into it: the deleted
        // off-curve forces the C0 -> C2 run to a line, while the other two
        // arcs are untouched.
        let result = delete_keeping_shape(&nodes, true, &[4, 5], 1.0).unwrap();
        assert_eq!(result.len(), 7);
        assert_eq!(result[0].nodetype, NodeType::OffCurve);
        assert_eq!(result[3].nodetype, NodeType::Line); // C2
        assert_eq!((result[3].x, result[3].y), (-100.0, 0.0));
        assert_eq!(result[6].nodetype, NodeType::Curve); // C3
        path_of(result, true).to_kurbo().unwrap();
    }

    #[test]
    fn deleting_a_line_node_collapses_to_a_line() {
        let nodes = vec![
            Node::new_line(0.0, 0.0),
            Node::new_line(100.0, 0.0),
            Node::new_line(100.0, 100.0),
            Node::new_line(0.0, 100.0),
        ];
        // Deleting one corner of a square leaves a triangle.
        let result = delete_keeping_shape(&nodes, true, &[1], 1.0).unwrap();
        assert_eq!(result.len(), 3);
        assert!(result.iter().all(|n| n.nodetype == NodeType::Line));
        let area = path_of(result, true).signed_area().unwrap();
        assert!((area - 5000.0).abs() < 1e-6);
    }

    #[test]
    fn open_path_curve_join_keeps_the_shape() {
        // A Move-to-C1 segment followed by two cubic segments which are exactly
        // the two halves of one cubic from (100, 50) to (300, 50); deleting
        // the middle node recovers the parent cubic.
        let nodes = vec![
            Node::new_move(0.0, 0.0),
            Node::new_offcurve(100.0 / 3.0, 50.0),
            Node::new_offcurve(200.0 / 3.0, 50.0),
            Node::new_curve(100.0, 50.0), // C1
            Node::new_offcurve(400.0 / 3.0, 75.0),
            Node::new_offcurve(500.0 / 3.0, 87.5),
            Node::new_curve(200.0, 87.5), // C2
            Node::new_offcurve(700.0 / 3.0, 87.5),
            Node::new_offcurve(800.0 / 3.0, 75.0),
            Node::new_curve(300.0, 50.0), // C3
        ];
        let result = delete_keeping_shape(&nodes, false, &[6], 1.0).unwrap();
        assert_eq!(result.len(), 7);
        assert_eq!(result[0].nodetype, NodeType::Move);
        assert_eq!(result[3].nodetype, NodeType::Curve); // C1
        assert_eq!(result[6].nodetype, NodeType::Curve); // C3
        assert_eq!((result[6].x, result[6].y), (300.0, 50.0));
        // The new off-curves sit where the parent cubic's control points were:
        // (500/3, 100) and (700/3, 100).
        assert!((result[4].x - 500.0 / 3.0).abs() < 1.0);
        assert!((result[4].y - 100.0).abs() < 1.0);
        assert!((result[5].x - 700.0 / 3.0).abs() < 1.0);
        assert!((result[5].y - 100.0).abs() < 1.0);
    }

    #[test]
    fn deleting_the_move_node_starts_the_path_at_the_next_node() {
        let nodes = vec![
            Node::new_move(0.0, 0.0),
            Node::new_line(100.0, 0.0),
            Node::new_line(100.0, 100.0),
        ];
        let result = delete_keeping_shape(&nodes, false, &[0], 1.0).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].nodetype, NodeType::Move);
        assert_eq!((result[0].x, result[0].y), (100.0, 0.0));
        assert_eq!(result[1].nodetype, NodeType::Line);
    }

    #[test]
    fn untouched_quadratic_segments_are_preserved() {
        // A line, a quadratic and a cubic; deleting an off-curve of the cubic
        // must not disturb the quadratic segment.
        let nodes = vec![
            Node::new_move(0.0, 0.0),
            Node::new_line(100.0, 0.0),
            Node::new_offcurve(150.0, 50.0),
            Node::new_qcurve(200.0, 0.0),
            Node::new_offcurve(250.0, 50.0),
            Node::new_offcurve(250.0, -50.0),
            Node::new_curve(300.0, 0.0),
        ];
        let result = delete_keeping_shape(&nodes, false, &[4], 1.0).unwrap();
        assert_eq!(result.len(), 5);
        assert_eq!(result[2].nodetype, NodeType::OffCurve); // the quad's off-curve
        assert_eq!(result[3].nodetype, NodeType::QCurve); // the quad survives
        assert_eq!(result[4].nodetype, NodeType::Line); // Curve(300, 0)
        assert_eq!((result[4].x, result[4].y), (300.0, 0.0));
        path_of(result, false).to_kurbo().unwrap();
    }

    #[test]
    fn deleting_every_on_curve_node_empties_the_path() {
        let nodes = quarter_circle();
        let result = delete_keeping_shape(&nodes, true, &[2, 5, 8, 11], 1.0).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn nothing_to_delete_returns_the_nodes_unchanged() {
        let nodes = quarter_circle();
        let result = delete_keeping_shape(&nodes, true, &[], 1.0).unwrap();
        assert_eq!(result, nodes);
        // Out-of-range indices are ignored.
        let result = delete_keeping_shape(&nodes, true, &[100, 200], 1.0).unwrap();
        assert_eq!(result, nodes);
    }
}
