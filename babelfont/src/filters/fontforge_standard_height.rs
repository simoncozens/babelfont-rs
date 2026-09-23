//! FontForge's estimate of a font's x-height and cap height.
//!
//! When a FontForge source does not state the OS/2 x-height or cap height, FontForge's
//! exporter computes it from the outlines (`SFXHeight` and `SFCapHeight`, which call
//! `SFStandardHeight` in `fontforge/splinefont.c`). It writes the two only into an
//! OS/2 table of version 2 or later, which a TrueType export gets only when the source
//! asks for it (see `setos2` in `fontforge/tottf.c`). This module reproduces that
//! computation, including the numeric tolerances FontForge applies when it decides
//! whether a curve is really a line, because the flat/curved decision changes the
//! result.
//!
//! The rule:
//!
//! 1. For each codepoint in a fixed list -- capitals for the cap height, a set of
//!    lowercase letters for the x-height -- find the glyph's highest point, and
//!    whether it is a flat top (a horizontal line), a round one (a curve) or a
//!    pointed one (a sloping line). References count, transformed.
//! 2. If any tops are flat, take the most frequent flat height, averaging the
//!    heights that tie for most frequent. Otherwise take the mean of the distinct
//!    round and pointed heights, each counted once.
//! 3. If the font has `BlueValues`, snap to the bottom of the nearest zone, provided
//!    it is closer than one hundredth of the em.
//!
//! The exporter stores the result in a signed 16-bit field, truncating towards zero,
//! and writes 0 when none of the glyphs exist.

use crate::{Font, Master, Shape};
use kurbo::{Affine, PathSeg, Point};
use std::collections::HashMap;

/// A marker used by FontForge's character lists: `a, RANGE, b` means `a..=b`.
const RANGE: u32 = 0x40ff_ffff;

/// `capheight_str` in FontForge's `splinefont.c`, including its RANGE markers.
const CAP_HEIGHT_CHARS: &[u32] = &[
    'A' as u32, RANGE, 'Z' as u32, 0x391, RANGE, 0x3a9, 0x402, 0x404, 0x405, 0x406, 0x408, RANGE,
    0x40b, 0x40f, RANGE, 0x418, 0x41a, 0x42f,
];

/// `xheight_str` in FontForge's `splinefont.c`, including its RANGE markers.
const X_HEIGHT_CHARS: &[u32] = &[
    'a' as u32, 'c' as u32, 'e' as u32, 'g' as u32, 'm' as u32, 'n' as u32, 'o' as u32, 'p' as u32,
    'q' as u32, 'r' as u32, 's' as u32, 'u' as u32, 'v' as u32, 'w' as u32, 'x' as u32, 'y' as u32,
    'z' as u32, 0x131, 0x3b3, 0x3b9, 0x3ba, 0x3bc, 0x3bd, 0x3c0, 0x3c3, 0x3c4, 0x3c5, 0x3c7, 0x3c8,
    0x3c9, 0x432, 0x433, 0x438, 0x43a, RANGE, 0x43f, 0x442, 0x443, 0x445, 0x44c, 0x44f, 0x459,
    0x45a,
];

/// How deep to follow references inside references.
const MAX_REFERENCE_DEPTH: usize = 8;

/// FontForge's `RE_NearZero` and `RE_Factor` in its double-precision configuration.
const RE_NEAR_ZERO: f64 = 1e-8;
const RE_FACTOR: f64 = 1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0 * 2.0;

/// How FontForge averaged the tops when none of them is flat.
///
/// FontForge's code divided the sum of the distinct tops by the number of glyphs until
/// commit 4d34d21ef866 (2012-05-14, "it was dividing by the wrong value"), and by the
/// number of distinct tops afterwards. A font exported by a build from before that
/// commit carries the smaller figure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CurveMean {
    /// Current FontForge: the sum of the distinct tops over their number.
    DistinctTops,
    /// FontForge built before 2012-05-14: the same sum over the number of glyphs.
    GlyphCount,
}

/// Which of the two heights to compute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StandardHeight {
    /// The OS/2 x-height.
    XHeight,
    /// The OS/2 cap height.
    CapHeight,
}

impl StandardHeight {
    fn chars(self) -> &'static [u32] {
        match self {
            StandardHeight::XHeight => X_HEIGHT_CHARS,
            StandardHeight::CapHeight => CAP_HEIGHT_CHARS,
        }
    }
}

/// The x-height and cap height FontForge's exporter writes for one master.
pub(crate) struct MeasuredHeights {
    pub(crate) x_height: i32,
    pub(crate) cap_height: i32,
}

/// Both heights FontForge's exporter writes for `master`.
pub(crate) fn exported_heights(font: &Font, master: &Master, mean: CurveMean) -> MeasuredHeights {
    MeasuredHeights {
        x_height: exported_height(font, master, StandardHeight::XHeight, mean),
        cap_height: exported_height(font, master, StandardHeight::CapHeight, mean),
    }
}

/// The value FontForge's exporter writes for `which`.
pub(crate) fn exported_height(
    font: &Font,
    master: &Master,
    which: StandardHeight,
    mean: CurveMean,
) -> i32 {
    // `os2->xHeight = (xh >= 0.0 ? xh : 0)`: into a short, so truncation, and 0
    // when none of the listed glyphs exist.
    match standard_height(font, master, which, mean) {
        Some(h) if h >= 0.0 => h as i32,
        _ => 0,
    }
}

/// How the top of a glyph is shaped, in FontForge's classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TopShape {
    Flat,
    Round,
    Pointy,
    Unknown,
}

/// The highest point of a set of outlines and the shape of the top there.
#[derive(Clone, Copy, Debug)]
struct Top {
    height: f64,
    shape: TopShape,
}

impl Top {
    fn none() -> Self {
        Top {
            height: -1e23,
            shape: TopShape::Unknown,
        }
    }
}

/// A height seen at the top of some glyphs, and how many of them.
struct Occurrence {
    height: f64,
    count: usize,
}

fn standard_height(
    font: &Font,
    master: &Master,
    which: StandardHeight,
    mean: CurveMean,
) -> Option<f64> {
    let by_codepoint = glyphs_by_primary_codepoint(font);
    let mut flats: Vec<Occurrence> = vec![];
    let mut curves: Vec<Occurrence> = vec![];
    for codepoint in expand(which.chars()) {
        let Some(name) = by_codepoint.get(&codepoint) else {
            continue;
        };
        let top = glyph_top(font, master, name);
        let bucket = match top.shape {
            TopShape::Flat => &mut flats,
            TopShape::Round | TopShape::Pointy => &mut curves,
            TopShape::Unknown => continue,
        };
        match bucket.iter_mut().find(|o| o.height == top.height) {
            Some(o) => o.count += 1,
            None => bucket.push(Occurrence {
                height: top.height,
                count: 1,
            }),
        }
    }
    let result = if flats.len() == 1 {
        flats[0].height
    } else if !flats.is_empty() {
        let most = flats.iter().map(|o| o.count).max().unwrap_or(0);
        let tied: Vec<f64> = flats
            .iter()
            .filter(|o| o.count == most)
            .map(|o| o.height)
            .collect();
        tied.iter().sum::<f64>() / tied.len() as f64
    } else if curves.is_empty() {
        return None;
    } else {
        // The sum of the distinct heights, each counted once however many glyphs
        // share it, over their number -- or, before 2012, over the number of glyphs.
        let divisor = match mean {
            CurveMean::DistinctTops => curves.len(),
            CurveMean::GlyphCount => curves.iter().map(|o| o.count).sum(),
        };
        curves.iter().map(|o| o.height).sum::<f64>() / divisor as f64
    };
    Some(snap_to_blue_zone(font, master, result))
}

/// FontForge's walk over a character list: `a, RANGE, b` is `a..=b`.
fn expand(list: &[u32]) -> Vec<u32> {
    let mut out = vec![];
    let mut i = 0;
    while i < list.len() {
        let low = list[i];
        let mut high = low;
        if i + 2 < list.len() && list[i + 1] == RANGE {
            high = list[i + 2];
            i += 2;
        }
        out.extend(low..=high);
        i += 1;
    }
    out
}

/// A glyph looked up by codepoint is the first glyph whose primary codepoint it is.
fn glyphs_by_primary_codepoint(font: &Font) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    for glyph in font.glyphs.iter() {
        if let Some(cp) = glyph.codepoints.first() {
            map.entry(*cp).or_insert_with(|| glyph.name.to_string());
        }
    }
    map
}

/// The bottom of the nearest `BlueValues` zone within a hundredth of the em, if any.
fn snap_to_blue_zone(font: &Font, master: &Master, height: f64) -> f64 {
    let Some(zones) = blue_values(font) else {
        return height;
    };
    let ascender = master
        .metrics
        .get(&crate::MetricType::Ascender)
        .copied()
        .unwrap_or(0);
    let descender = master
        .metrics
        .get(&crate::MetricType::Descender)
        .copied()
        .unwrap_or(0);
    let mut best = height;
    let mut best_distance = f64::from(ascender - descender) / 100.0;
    // Zones are (bottom, top) pairs; only the bottoms are candidates.
    for bottom in zones.iter().step_by(2) {
        let distance = (bottom - height).abs();
        if distance < best_distance {
            best = *bottom;
            best_distance = distance;
        }
    }
    best
}

/// The numbers in the source's `BlueValues`, from a FontForge private dictionary.
fn blue_values(font: &Font) -> Option<Vec<f64>> {
    let lines = font
        .format_specific
        .get("sfd.private_section")?
        .as_array()?;
    for line in lines.iter().filter_map(|l| l.as_str()) {
        let mut fields = line.trim().splitn(3, ' ');
        if fields.next() != Some("BlueValues") {
            continue;
        }
        let _length = fields.next();
        let value = fields.next().unwrap_or("");
        let numbers = value
            .trim_matches(|c: char| c == '[' || c == ']' || c.is_whitespace())
            .split_whitespace()
            .map_while(|n| n.parse::<f64>().ok())
            .collect();
        return Some(numbers);
    }
    None
}

/// `SCMaxHeight`: a glyph's own outlines, then each reference. A reference replaces
/// the running top when it is higher, or equally high and flat.
fn glyph_top(font: &Font, master: &Master, name: &str) -> Top {
    let Some(layer) = font.master_layer_for(name, master) else {
        return Top::none();
    };
    let mut own = vec![];
    for shape in layer.shapes.iter() {
        if let Shape::Path(path) = shape {
            push_segments(&mut own, path, Affine::IDENTITY);
        }
    }
    let mut top = spline_set_top(&own);
    for shape in layer.shapes.iter() {
        if let Shape::Component(component) = shape {
            let mut flattened = vec![];
            flatten_reference(
                font,
                master,
                &component.reference,
                component.transform.as_affine(),
                0,
                &mut flattened,
            );
            let reference = spline_set_top(&flattened);
            if reference.height > top.height
                || (reference.height == top.height && reference.shape == TopShape::Flat)
            {
                top = reference;
            }
        }
    }
    top
}

/// A reference's outlines as FontForge measures them: the referenced glyph's own
/// outlines followed by those of its own references, all transformed, as one list.
fn flatten_reference(
    font: &Font,
    master: &Master,
    name: &str,
    transform: Affine,
    depth: usize,
    out: &mut Vec<Segment>,
) {
    let Some(layer) = font.master_layer_for(name, master) else {
        return;
    };
    for shape in layer.shapes.iter() {
        if let Shape::Path(path) = shape {
            push_segments(out, path, transform);
        }
    }
    if depth >= MAX_REFERENCE_DEPTH {
        return;
    }
    for shape in layer.shapes.iter() {
        if let Shape::Component(component) = shape {
            flatten_reference(
                font,
                master,
                &component.reference,
                transform * component.transform.as_affine(),
                depth + 1,
                out,
            );
        }
    }
}

/// One outline segment, with its control points as FontForge stores them: a line
/// has control points at its ends, a quadratic has one control point written twice.
#[derive(Clone, Copy, Debug)]
struct Segment {
    from: Point,
    next_cp: Point,
    prev_cp: Point,
    to: Point,
    quadratic: bool,
}

fn push_segments(out: &mut Vec<Segment>, path: &crate::Path, transform: Affine) {
    let Ok(bez) = path.to_kurbo() else {
        return;
    };
    for seg in (transform * bez).segments() {
        let segment = match seg {
            PathSeg::Line(l) => Segment {
                from: l.p0,
                next_cp: l.p0,
                prev_cp: l.p1,
                to: l.p1,
                quadratic: false,
            },
            PathSeg::Quad(q) => Segment {
                from: q.p0,
                next_cp: q.p1,
                prev_cp: q.p1,
                to: q.p2,
                quadratic: true,
            },
            PathSeg::Cubic(c) => Segment {
                from: c.p0,
                next_cp: c.p1,
                prev_cp: c.p2,
                to: c.p3,
                quadratic: false,
            },
        };
        // FontForge has no spline for a closing edge of zero length.
        if segment.from == segment.to && segment.from == segment.next_cp {
            continue;
        }
        out.push(segment);
    }
}

/// `SPLMaxHeight`: the highest point of a list of splines and its shape.
fn spline_set_top(segments: &[Segment]) -> Top {
    let mut top = Top::none();
    for s in segments {
        let spline = Spline::new(s);
        if !(s.from.y >= top.height
            || s.to.y >= top.height
            || s.next_cp.y > top.height
            || s.prev_cp.y > top.height)
        {
            continue;
        }
        if !spline.linear {
            if s.from.y > top.height {
                top = round(s.from.y);
            }
            if s.to.y > top.height {
                top = round(s.to.y);
            }
            for t in find_extrema(&spline.y).into_iter().flatten() {
                let y = spline.y.at(t);
                if y > top.height {
                    top = round(y);
                }
            }
        } else if s.from.y == s.to.y {
            if s.from.y >= top.height {
                top = Top {
                    height: s.from.y,
                    shape: TopShape::Flat,
                };
            }
        } else {
            if s.from.y > top.height {
                top = pointy(s.from.y);
            }
            if s.to.y > top.height {
                top = pointy(s.to.y);
            }
        }
    }
    top
}

fn round(height: f64) -> Top {
    Top {
        height,
        shape: TopShape::Round,
    }
}

fn pointy(height: f64) -> Top {
    Top {
        height,
        shape: TopShape::Pointy,
    }
}

/// One coordinate of a spline as a cubic polynomial in t: ((a*t + b)*t + c)*t + d.
#[derive(Clone, Copy, Debug)]
struct Polynomial {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

impl Polynomial {
    fn at(&self, t: f64) -> f64 {
        ((self.a * t + self.b) * t + self.c) * t + self.d
    }

    fn line(from: f64, to: f64) -> Self {
        Polynomial {
            a: 0.0,
            b: 0.0,
            c: to - from,
            d: from,
        }
    }
}

/// A spline after FontForge's `SplineRefigure3` / `SplineRefigure2` and
/// `SplineIsLinear`: its polynomials, and whether FontForge treats it as a line.
struct Spline {
    x: Polynomial,
    y: Polynomial,
    linear: bool,
}

impl Spline {
    fn new(s: &Segment) -> Self {
        let no_control_points = if s.quadratic {
            s.next_cp == s.from || s.prev_cp == s.to
        } else {
            s.next_cp == s.from && s.prev_cp == s.to
        };
        let mut spline = if no_control_points {
            Spline {
                x: Polynomial::line(s.from.x, s.to.x),
                y: Polynomial::line(s.from.y, s.to.y),
                linear: true,
            }
        } else {
            let x = polynomial(s.from.x, s.next_cp.x, s.prev_cp.x, s.to.x, s.quadratic);
            let y = polynomial(s.from.y, s.next_cp.y, s.prev_cp.y, s.to.y, s.quadratic);
            let linear = x.a == 0.0 && y.a == 0.0 && x.b == 0.0 && y.b == 0.0;
            Spline { x, y, linear }
        };
        if !spline.linear && is_linear(s, &spline) {
            spline.linear = true;
        }
        if spline.linear {
            spline.x = Polynomial::line(s.from.x, s.to.x);
            spline.y = Polynomial::line(s.from.y, s.to.y);
        }
        spline
    }
}

fn polynomial(from: f64, next_cp: f64, prev_cp: f64, to: f64, quadratic: bool) -> Polynomial {
    let (mut a, mut b, mut c) = if quadratic {
        let c = 2.0 * (next_cp - from);
        (0.0, to - from - c, c)
    } else {
        let c = 3.0 * (next_cp - from);
        let b = 3.0 * (prev_cp - next_cp) - c;
        (to - from - c - b, b, c)
    };
    if real_near(c, 0.0) {
        c = 0.0;
    }
    if real_near(b, 0.0) {
        b = 0.0;
    }
    if real_near(a, 0.0) {
        a = 0.0;
    }
    if a != 0.0
        && (within_16_rounding_errors(a + from, from) || within_16_rounding_errors(a + to, to))
    {
        a = 0.0;
    }
    Polynomial { a, b, c, d: from }
}

/// `SplineIsLinear`: a spline whose control points lie on the chord between its end
/// points is a line, as long as it does not double back on itself.
fn is_linear(s: &Segment, spline: &Spline) -> bool {
    let (from, next_cp, prev_cp, to) = (s.from, s.next_cp, s.prev_cp, s.to);
    if real_near(from.x, to.x) {
        let on_line = real_near(from.x, next_cp.x) && real_near(from.x, prev_cp.x);
        if on_line && !between(from.y, next_cp.y, prev_cp.y, to.y) {
            return min_max_within(s, spline);
        }
        on_line
    } else if real_near(from.y, to.y) {
        let on_line = real_near(from.y, next_cp.y) && real_near(from.y, prev_cp.y);
        if on_line && !between(from.x, next_cp.x, prev_cp.x, to.x) {
            return min_max_within(s, spline);
        }
        on_line
    } else {
        let t1 = (next_cp.y - from.y) / (to.y - from.y);
        let t2 = (next_cp.x - from.x) / (to.x - from.x);
        let t3 = (to.y - prev_cp.y) / (to.y - from.y);
        let t4 = (to.x - prev_cp.x) / (to.x - from.x);
        let on_line = (within_16_rounding_errors(t1, t2)
            || (real_approx(t1, 0.0) && real_approx(t2, 0.0)))
            && (within_16_rounding_errors(t3, t4)
                || (real_approx(t3, 0.0) && real_approx(t4, 0.0)));
        if on_line && [t1, t2, t3, t4].iter().any(|t| *t < 0.0 || *t > 1.0) {
            return min_max_within(s, spline);
        }
        on_line
    }
}

/// Both control points lie between the two ends, in either direction.
fn between(from: f64, next_cp: f64, prev_cp: f64, to: f64) -> bool {
    (next_cp >= from && next_cp <= to && prev_cp >= from && prev_cp <= to)
        || (next_cp <= from && next_cp >= to && prev_cp <= from && prev_cp >= to)
}

/// `MinMaxWithin`: a collinear spline is still a line if its extrema along the line
/// stay between its ends.
fn min_max_within(s: &Segment, spline: &Spline) -> bool {
    let along_y = (s.to.x - s.from.x).abs() < (s.to.y - s.from.y).abs();
    let (poly, from, to) = if along_y {
        (&spline.y, s.from.y, s.to.y)
    } else {
        (&spline.x, s.from.x, s.to.x)
    };
    for t in find_extrema(poly).into_iter().flatten() {
        let w = poly.at(t);
        if real_near(w, to) || real_near(w, from) {
            continue;
        }
        if (w < to && w < from) || (w > to && w > from) {
            return false;
        }
    }
    true
}

/// `SplineFindExtrema`: the parameters strictly inside (0, 1) where the derivative
/// is zero, smallest first.
fn find_extrema(p: &Polynomial) -> [Option<f64>; 2] {
    if p.a != 0.0 {
        let discriminant = 4.0 * p.b * p.b - 12.0 * p.a * p.c;
        if discriminant < 0.0 {
            return [None, None];
        }
        let root = discriminant.sqrt();
        let mut t1 = (-2.0 * p.b - root) / (6.0 * p.a);
        let mut t2 = (-2.0 * p.b + root) / (6.0 * p.a);
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        let mut second = if t1 == t2 { None } else { Some(t2) };
        let snap = |t: f64| {
            if real_near(t, 0.0) {
                0.0
            } else if real_near(t, 1.0) {
                1.0
            } else {
                t
            }
        };
        let t1 = snap(t1);
        second = second.map(snap).filter(|t| *t > 0.0 && *t < 1.0);
        if t1 > 0.0 && t1 < 1.0 {
            [Some(t1), second]
        } else {
            [second, None]
        }
    } else if p.b != 0.0 {
        let t = -p.c / (2.0 * p.b);
        if t > 0.0 && t < 1.0 {
            [Some(t), None]
        } else {
            [None, None]
        }
    } else {
        [None, None]
    }
}

fn real_near(a: f64, b: f64) -> bool {
    if a == 0.0 {
        return b > -1e-8 && b < 1e-8;
    }
    if b == 0.0 {
        return a > -1e-8 && a < 1e-8;
    }
    let d = a - b;
    d > -1e-6 && d < 1e-6
}

fn real_approx(a: f64, b: f64) -> bool {
    if a == 0.0 {
        return b < 0.0001 && b > -0.0001;
    }
    if b == 0.0 {
        return a < 0.0001 && a > -0.0001;
    }
    let ratio = a / b;
    (0.95..=1.05).contains(&ratio)
}

fn within_16_rounding_errors(v1: f64, v2: f64) -> bool {
    let product = v1 * v2;
    if product < 0.0 {
        return false;
    }
    if product == 0.0 {
        let other = if v1 == 0.0 { v2 } else { v1 };
        return other < RE_NEAR_ZERO && other > -RE_NEAR_ZERO;
    }
    if v1 > 0.0 {
        if v1 > v2 {
            v1 - v2 < v1 / (RE_FACTOR / 16.0)
        } else {
            v2 - v1 < v2 / (RE_FACTOR / 16.0)
        }
    } else if v1 < v2 {
        v1 - v2 > v1 / (RE_FACTOR / 16.0)
    } else {
        v2 - v1 > v2 / (RE_FACTOR / 16.0)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Glyph, Layer, LayerType, MetricType, Node, NodeType, Path};

    fn node(x: f64, y: f64, nodetype: NodeType) -> Node {
        Node {
            x,
            y,
            nodetype,
            ..Default::default()
        }
    }

    /// A closed rectangle whose top edge is a horizontal line at `top`.
    fn flat_topped(left: f64, right: f64, top: f64) -> Path {
        Path {
            nodes: vec![
                node(left, 0.0, NodeType::Line),
                node(right, 0.0, NodeType::Line),
                node(right, top, NodeType::Line),
                node(left, top, NodeType::Line),
            ],
            closed: true,
            ..Default::default()
        }
    }

    /// A closed shape whose top is a cubic arch peaking at `peak`.
    fn arched(left: f64, right: f64, peak: f64) -> Path {
        // A symmetric cubic from (right, s) to (left, s) with both handles at
        // height h peaks at s + 0.75 * (h - s).
        let shoulder = peak - 100.0;
        let handle = shoulder + (peak - shoulder) / 0.75;
        Path {
            nodes: vec![
                node(left, 0.0, NodeType::Line),
                node(right, 0.0, NodeType::Line),
                node(right, shoulder, NodeType::Line),
                node(right, handle, NodeType::OffCurve),
                node(left, handle, NodeType::OffCurve),
                node(left, shoulder, NodeType::Curve),
            ],
            closed: true,
            ..Default::default()
        }
    }

    fn font_with_glyphs(glyphs: Vec<(char, Path)>) -> Font {
        let mut master = Master {
            id: "m".to_string(),
            ..Default::default()
        };
        master.metrics.insert(MetricType::Ascender, 800);
        master.metrics.insert(MetricType::Descender, -200);
        let mut font = Font::new();
        font.masters.push(master);
        for (ch, path) in glyphs {
            let mut glyph = Glyph::new(&ch.to_string());
            glyph.codepoints = vec![ch as u32];
            let mut layer = Layer::new(500.0);
            layer.master = LayerType::DefaultForMaster("m".to_string());
            layer.shapes.push(Shape::Path(path));
            glyph.layers.push(layer);
            font.glyphs.push(glyph);
        }
        font
    }

    fn height(font: &Font, which: StandardHeight) -> i32 {
        exported_height(font, &font.masters[0], which, CurveMean::DistinctTops)
    }

    #[test]
    fn test_the_most_frequent_flat_top_wins() {
        let font = font_with_glyphs(vec![
            ('H', flat_topped(0.0, 400.0, 700.0)),
            ('E', flat_topped(0.0, 400.0, 700.0)),
            ('T', flat_topped(0.0, 400.0, 710.0)),
            ('O', arched(0.0, 400.0, 715.0)),
        ]);
        assert_eq!(height(&font, StandardHeight::CapHeight), 700);
    }

    #[test]
    fn test_a_tie_for_the_mode_is_averaged() {
        let font = font_with_glyphs(vec![
            ('x', flat_topped(0.0, 400.0, 500.0)),
            ('z', flat_topped(0.0, 400.0, 511.0)),
        ]);
        assert_eq!(height(&font, StandardHeight::XHeight), 505);
    }

    #[test]
    fn test_without_flat_tops_the_mean_of_distinct_heights_is_used() {
        // Two glyphs share 600; it is counted once: (600 + 612) / 2 = 606.
        let font = font_with_glyphs(vec![
            ('o', arched(0.0, 400.0, 600.0)),
            ('c', arched(0.0, 400.0, 600.0)),
            ('e', arched(0.0, 400.0, 612.0)),
        ]);
        assert_eq!(height(&font, StandardHeight::XHeight), 606);
    }

    #[test]
    fn test_before_2012_the_curve_sum_is_divided_by_the_glyph_count() {
        // Distinct tops 600 (two glyphs) and 612 (one glyph): 1212 / 3 = 404.
        let font = font_with_glyphs(vec![
            ('o', arched(0.0, 400.0, 600.0)),
            ('c', arched(0.0, 400.0, 600.0)),
            ('e', arched(0.0, 400.0, 612.0)),
        ]);
        let m = &font.masters[0];
        assert_eq!(
            exported_height(&font, m, StandardHeight::XHeight, CurveMean::GlyphCount),
            404
        );
        assert_eq!(
            exported_height(&font, m, StandardHeight::XHeight, CurveMean::DistinctTops),
            606
        );
    }

    #[test]
    fn test_a_curve_with_handles_on_its_chord_is_a_flat_top() {
        // A horizontal top drawn as a curve whose handles sit on the line between
        // its ends is a line to FontForge, so this is a flat top at 480 and wins
        // over the higher round one.
        let flat_curve = Path {
            nodes: vec![
                node(0.0, 0.0, NodeType::Line),
                node(400.0, 0.0, NodeType::Line),
                node(400.0, 480.0, NodeType::Line),
                node(300.0, 480.0, NodeType::OffCurve),
                node(100.0, 480.0, NodeType::OffCurve),
                node(0.0, 480.0, NodeType::Curve),
            ],
            closed: true,
            ..Default::default()
        };
        let font = font_with_glyphs(vec![('x', flat_curve), ('o', arched(0.0, 400.0, 495.0))]);
        assert_eq!(height(&font, StandardHeight::XHeight), 480);
    }

    #[test]
    fn test_snaps_to_the_nearest_blue_zone_bottom() {
        let mut font = font_with_glyphs(vec![('x', flat_topped(0.0, 400.0, 505.0))]);
        font.format_specific.insert(
            "sfd.private_section".to_string(),
            serde_json::Value::Array(vec![serde_json::Value::String(
                "BlueValues 23 [-12 0 500 512 700 712]".to_string(),
            )]),
        );
        assert_eq!(height(&font, StandardHeight::XHeight), 500);
    }

    #[test]
    fn test_a_zone_further_than_a_hundredth_of_the_em_is_ignored() {
        let mut font = font_with_glyphs(vec![('x', flat_topped(0.0, 400.0, 489.0))]);
        font.format_specific.insert(
            "sfd.private_section".to_string(),
            serde_json::Value::Array(vec![serde_json::Value::String(
                "BlueValues 11 [-12 0 500 512]".to_string(),
            )]),
        );
        // em 1000, so a zone must be closer than 10 units.
        assert_eq!(height(&font, StandardHeight::XHeight), 489);
    }

    #[test]
    fn test_no_glyphs_gives_zero() {
        let font = font_with_glyphs(vec![('1', flat_topped(0.0, 400.0, 700.0))]);
        assert_eq!(height(&font, StandardHeight::CapHeight), 0);
    }

    #[test]
    fn test_character_ranges_are_inclusive() {
        let caps = expand(CAP_HEIGHT_CHARS);
        assert!(caps.contains(&('A' as u32)) && caps.contains(&('Z' as u32)));
        assert!(caps.contains(&0x3a9) && !caps.contains(&0x3aa));
        // 0x41a and 0x42f are listed singly, not as a range.
        assert!(caps.contains(&0x41a) && !caps.contains(&0x41b) && caps.contains(&0x42f));
    }
}
