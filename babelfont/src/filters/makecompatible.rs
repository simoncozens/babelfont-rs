use std::fmt::Debug;

use crate::{filters::FontFilter, BabelfontError, LayerType, NodeType, Path};
use kurbo::{BezPath, ParamCurve, ParamCurveArclen, ParamCurveCurvature, PathSeg, Shape, Vec2};

fn rotate_closed_path_to_bottom_left(path: &mut Path) {
    if !path.closed || path.nodes.is_empty() {
        return;
    }

    let mut best_ix: Option<usize> = None;
    for (ix, node) in path.nodes.iter().enumerate() {
        if node.nodetype == NodeType::OffCurve || node.nodetype == NodeType::Move {
            continue;
        }
        match best_ix {
            None => best_ix = Some(ix),
            Some(bix) => {
                let b = &path.nodes[bix];
                if (node.y, node.x) < (b.y, b.x) {
                    best_ix = Some(ix);
                }
            }
        }
    }
    if let Some(ix) = best_ix {
        path.nodes.rotate_left(ix);
    }
}

fn path_shape_indices(layer: &crate::Layer) -> Vec<usize> {
    layer
        .shapes
        .iter()
        .enumerate()
        .filter_map(|(ix, sh)| sh.is_path().then_some(ix))
        .collect()
}

fn get_path_mut(
    glyph: &mut crate::Glyph,
    layer_ix: usize,
    path_shape_ix: usize,
) -> Option<&mut Path> {
    glyph.layers[layer_ix].shapes[path_shape_ix].as_path_mut()
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
enum LandmarkType {
    #[default]
    None,
    HorizontalExtrema,
    VerticalExtrema,
    InflectionPoint,
    Corner,
}

#[derive(Debug, Clone)]
struct AnnotatedSegment {
    seg: PathSeg,
    landmark: LandmarkType, // Tells us something about the *start* of the segment
    heading: Vec2,          // Unit vector in the direction of the segment at the start point
    path_length_at_start: f64, // Total path length up to the start of this segment from 0 to 1, used for distributing new nodes
}

#[derive(Clone, Copy)]
struct SoftSignature {
    kind: u8,
    landmark: LandmarkType,
    heading_angle: f64,
    path_time: f64,
}

impl Debug for SoftSignature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({:?})",
            match self.kind {
                0 => "Line",
                1 => "Quad",
                2 => "Cubic",
                _ => "Unknown",
            },
            self.landmark
        )
    }
}

impl AnnotatedSegment {
    fn signature(&self) -> SoftSignature {
        let kind = match self.seg {
            PathSeg::Line(_) => 0,
            PathSeg::Quad(_) => 1,
            PathSeg::Cubic(_) => 2,
        };
        SoftSignature {
            kind,
            landmark: self.landmark,
            heading_angle: self.heading.angle(),
            path_time: self.path_length_at_start,
        }
    }
}

fn angle_delta(a: f64, b: f64) -> f64 {
    let mut d = (a - b).abs();
    let two_pi = std::f64::consts::TAU;
    while d > two_pi {
        d -= two_pi;
    }
    d.min(two_pi - d)
}

fn soft_signature_match(a: SoftSignature, b: SoftSignature) -> bool {
    a.kind == b.kind && a.landmark == b.landmark
}

fn heading_at_start(seg: &PathSeg) -> Vec2 {
    match seg {
        PathSeg::Line(l) => {
            let v = l.p1 - l.p0;
            if v.length() > 0.0 {
                v.normalize()
            } else {
                Vec2::ZERO
            }
        }
        PathSeg::Quad(q) => {
            let mut v = q.p1 - q.p0;
            if v.length() <= 0.0 {
                v = q.p2 - q.p0;
            }
            if v.length() > 0.0 {
                v.normalize()
            } else {
                Vec2::ZERO
            }
        }
        PathSeg::Cubic(c) => {
            let mut v = c.p1 - c.p0;
            if v.length() <= 0.0 {
                v = c.p2 - c.p0;
            }
            if v.length() <= 0.0 {
                v = c.p3 - c.p0;
            }
            if v.length() > 0.0 {
                v.normalize()
            } else {
                Vec2::ZERO
            }
        }
    }
}

fn annotate_segments(segments: Vec<PathSeg>) -> Vec<AnnotatedSegment> {
    let mut segs: Vec<AnnotatedSegment> = segments
        .into_iter()
        .map(|seg| AnnotatedSegment {
            seg,
            landmark: LandmarkType::None,
            heading: Vec2::ZERO,
            path_length_at_start: 0.0,
        })
        .collect();
    let mut todo_list = Vec::new();
    let mut headings = Vec::with_capacity(segs.len());
    let mut lengths = Vec::with_capacity(segs.len());
    let mut current_path_length = 0.0;
    for ix in 0..segs.len() {
        let cur = &segs[ix];
        let prev = &segs[(ix + segs.len() - 1) % segs.len()];
        lengths.push(current_path_length);
        match cur.seg {
            PathSeg::Line(l) => {
                if let PathSeg::Line(p) = prev.seg {
                    // A corner unless they're colinear
                    let v1 = l.p1 - l.p0;
                    let v2 = p.p1 - p.p0;
                    if v1.length() > 0.0 && v2.length() > 0.0 {
                        let angle1 = v1.angle();
                        let angle2 = v2.angle();
                        let angle = (angle1 - angle2).abs();
                        if angle < 0.01 || (std::f64::consts::PI - angle).abs() < 0.01 {
                            // Colinear, not a corner
                        } else {
                            todo_list.push((ix, LandmarkType::Corner));
                        }
                    }
                } else {
                    todo_list.push((ix, LandmarkType::Corner));
                }
                let v = l.p1 - l.p0;
                if v.length() > 0.0 {
                    headings.push(v.normalize());
                } else {
                    headings.push(Vec2::ZERO);
                }
                current_path_length += v.length();
            }
            PathSeg::Quad(_) => {
                if matches!(prev.seg, PathSeg::Line(_)) {
                    todo_list.push((ix, LandmarkType::Corner));
                }
                headings.push(heading_at_start(&cur.seg));
                current_path_length += cur.seg.arclen(0.1);
            }
            PathSeg::Cubic(c) => {
                if matches!(prev.seg, PathSeg::Line(_)) {
                    todo_list.push((ix, LandmarkType::Corner));
                }
                if let PathSeg::Cubic(prev) = prev.seg {
                    if prev.curvature(0.9).signum() != c.curvature(0.1).signum() {
                        todo_list.push((ix, LandmarkType::InflectionPoint));
                    }
                    if (c.p0.x - c.p1.x).abs() < 0.01 && (prev.p2.x - prev.p3.x).abs() < 0.01 {
                        todo_list.push((ix, LandmarkType::VerticalExtrema));
                    }
                    if (c.p0.y - c.p1.y).abs() < 0.01 && (prev.p2.y - prev.p3.y).abs() < 0.01 {
                        todo_list.push((ix, LandmarkType::HorizontalExtrema));
                    }
                }
                headings.push(heading_at_start(&cur.seg));
                current_path_length += c.arclen(0.1);
            }
        }
    }
    for (ix, landmark) in todo_list {
        segs[ix].landmark = landmark;
    }
    for (ix, heading) in headings.into_iter().enumerate() {
        segs[ix].heading = heading;
    }
    for (ix, path_length) in lengths.into_iter().enumerate() {
        segs[ix].path_length_at_start = if current_path_length > 0.0 {
            path_length / current_path_length
        } else {
            0.0
        };
    }
    segs
}

fn signatures(segs: &[AnnotatedSegment]) -> Vec<SoftSignature> {
    segs.iter().map(|s| s.signature()).collect()
}

fn scs_soft(a: &[SoftSignature], b: &[SoftSignature]) -> Vec<SoftSignature> {
    let m = a.len();
    let n = b.len();
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            dp[i][j] = if soft_signature_match(a[i], b[j]) {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    let mut i = 0;
    let mut j = 0;
    let mut out = Vec::new();
    while i < m && j < n {
        if soft_signature_match(a[i], b[j]) {
            out.push(a[i]);
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            out.push(a[i]);
            i += 1;
        } else {
            out.push(b[j]);
            j += 1;
        }
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
    out
}

fn insertion_distance_soft(sig: &[SoftSignature], target: &[SoftSignature]) -> Option<usize> {
    let m = sig.len();
    let n = target.len();
    let inf = usize::MAX / 4;
    let mut dp = vec![vec![inf; n + 1]; m + 1];

    dp[m][n] = 0;
    for j in (0..n).rev() {
        dp[m][j] = n - j;
    }
    for i in (0..m).rev() {
        dp[i][n] = inf;
    }
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            let insert_cost = 1usize.saturating_add(dp[i][j + 1]);
            let mut best = insert_cost;
            if soft_signature_match(sig[i], target[j]) {
                best = best.min(dp[i + 1][j + 1]);
            }
            dp[i][j] = best;
        }
    }

    (dp[0][0] < inf).then_some(dp[0][0])
}

fn first_required_insertion_soft(
    sig: &[SoftSignature],
    target: &[SoftSignature],
) -> Option<(usize, SoftSignature)> {
    if sig.len() >= target.len()
        && sig
            .iter()
            .zip(target)
            .all(|(a, b)| soft_signature_match(*a, *b))
    {
        return None;
    }

    let m = sig.len();
    let n = target.len();
    let inf = usize::MAX / 4;
    let mut dp = vec![vec![inf; n + 1]; m + 1];

    dp[m][n] = 0;
    for j in (0..n).rev() {
        dp[m][j] = n - j;
    }
    for i in (0..m).rev() {
        dp[i][n] = inf;
    }
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            let insert_cost = 1usize.saturating_add(dp[i][j + 1]);
            let mut best = insert_cost;
            if soft_signature_match(sig[i], target[j]) {
                best = best.min(dp[i + 1][j + 1]);
            }
            dp[i][j] = best;
        }
    }
    if dp[0][0] >= inf {
        return (m < n).then_some((0, target[0]));
    }

    let mut i = 0usize;
    let mut j = 0usize;
    while i < m && j < n {
        let insert_cost = 1usize.saturating_add(dp[i][j + 1]);
        let match_cost = if soft_signature_match(sig[i], target[j]) {
            dp[i + 1][j + 1]
        } else {
            inf
        };
        if insert_cost < match_cost {
            return Some((i, target[j]));
        }
        if match_cost < inf {
            i += 1;
            j += 1;
            continue;
        }
        return Some((i, target[j]));
    }
    (j < n).then_some((i, target[j]))
}

fn cyclic_distance(a: usize, b: usize, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let d = a.abs_diff(b) % n;
    d.min(n - d)
}

fn split_segment(seg: PathSeg) -> (PathSeg, PathSeg) {
    match seg {
        PathSeg::Line(l) => {
            let mid = l.p0.midpoint(l.p1);
            (
                PathSeg::Line(kurbo::Line::new(l.p0, mid)),
                PathSeg::Line(kurbo::Line::new(mid, l.p1)),
            )
        }
        PathSeg::Quad(q) => {
            let (a, b) = q.subdivide();
            (PathSeg::Quad(a), PathSeg::Quad(b))
        }
        PathSeg::Cubic(c) => {
            let (a, b) = c.subdivide();
            (PathSeg::Cubic(a), PathSeg::Cubic(b))
        }
    }
}

fn segment_length(seg: &PathSeg) -> f64 {
    match seg {
        PathSeg::Line(l) => (l.p1 - l.p0).length(),
        PathSeg::Quad(q) => q.arclen(0.1),
        PathSeg::Cubic(c) => c.arclen(0.1),
    }
}

fn choose_split_segment(
    segs: &[AnnotatedSegment],
    sig: &[SoftSignature],
    target: &[SoftSignature],
    wanted: SoftSignature,
    insert_before: usize,
) -> usize {
    let n = segs.len();
    if n == 0 {
        return 0;
    }

    let preferred_split = if insert_before == 0 || insert_before > n {
        n - 1
    } else {
        insert_before - 1
    };

    let mut best = preferred_split;
    let mut best_tuple = (
        usize::MAX,
        usize::MAX,
        f64::INFINITY,
        usize::MAX,
        f64::INFINITY,
    );
    let mut found_viable = false;
    for cand in 0..n {
        if segment_length(&segs[cand].seg) <= 1e-6 {
            continue;
        }
        let mut sim = Vec::with_capacity(n + 1);
        sim.extend_from_slice(&sig[..(cand + 1)]);
        sim.push(sig[cand]);
        sim.extend_from_slice(&sig[(cand + 1)..]);

        let Some(cost) = insertion_distance_soft(&sim, target) else {
            continue;
        };
        let kind_penalty = usize::from(sig[cand].kind != wanted.kind);
        let landmark_penalty = usize::from(
            !(sig[cand].landmark == wanted.landmark
                || sig[cand].landmark == LandmarkType::None
                || wanted.landmark == LandmarkType::None),
        );
        let time_delta = (sig[cand].path_time - wanted.path_time).abs();
        let dist = cyclic_distance(cand, preferred_split, n);
        let heading_penalty = angle_delta(sig[cand].heading_angle, wanted.heading_angle);
        let tuple = (
            cost,
            kind_penalty + landmark_penalty,
            time_delta,
            dist,
            heading_penalty,
        );
        if tuple < best_tuple {
            found_viable = true;
            best_tuple = tuple;
            best = cand;
        }
    }
    if !found_viable {
        preferred_split
    } else {
        best
    }
}

fn augment_to_target(
    segs: &[AnnotatedSegment],
    target: &[SoftSignature],
) -> Result<Vec<AnnotatedSegment>, BabelfontError> {
    let mut cur = segs.to_vec();
    let mut guard = 0usize;
    while guard < target.len() + 16 {
        guard += 1;
        let sig = signatures(&cur);
        if sig.len() == target.len()
            && sig
                .iter()
                .zip(target.iter())
                .all(|(a, b)| soft_signature_match(*a, *b))
        {
            return Ok(cur);
        }

        let Some((insert_before, wanted)) = first_required_insertion_soft(&sig, target) else {
            return Ok(cur);
        };

        if cur.is_empty() {
            return Err(BabelfontError::FilterError(
                "makecompatible cannot split empty path".to_string(),
            ));
        }
        let split_ix = choose_split_segment(&cur, &sig, target, wanted, insert_before);
        let (a, b) = split_segment(cur[split_ix].seg);

        let mut rebuilt = Vec::with_capacity(cur.len() + 1);
        for (ix, s) in cur.iter().enumerate() {
            if ix == split_ix {
                rebuilt.push(a);
                rebuilt.push(b);
            } else {
                rebuilt.push(s.seg);
            }
        }
        cur = annotate_segments(rebuilt);
    }
    Err(BabelfontError::FilterError(
        "makecompatible soft diff failed to converge".to_string(),
    ))
}

fn make_all_compatible(
    paths: &[BezPath],
    glyph_name: &str,
) -> Result<Vec<BezPath>, BabelfontError> {
    log::info!(
        "Making {} paths compatible for glyph '{}'",
        paths.len(),
        glyph_name
    );
    let mut segs_vec = paths
        .iter()
        .map(|p| annotate_segments(p.segments().collect::<Vec<_>>()))
        .collect::<Vec<_>>();

    let mut rounds = 0usize;
    while rounds < paths.len() {
        for first in 0..segs_vec.len() {
            for second in (first + 1)..segs_vec.len() {
                let first_sigs = signatures(&segs_vec[first]);
                let second_sigs = signatures(&segs_vec[second]);
                if first_sigs.len() == second_sigs.len()
                    && first_sigs
                        .iter()
                        .zip(&second_sigs)
                        .all(|(a, b)| soft_signature_match(*a, *b))
                {
                    break;
                }
                // Only do one round, assuming two paths for now.
                let (new_first, new_second) =
                    align(&segs_vec[first], &segs_vec[second], glyph_name, rounds)?;
                segs_vec[first] = new_first;
                segs_vec[second] = new_second;
            }
        }
        rounds += 1;
    }

    // Put back
    let new_paths = segs_vec
        .iter()
        .map(|segs| {
            let mut p = BezPath::new();
            if segs.is_empty() {
                return p;
            }
            p.move_to(segs[0].seg.start());
            for seg in segs {
                match seg.seg {
                    PathSeg::Line(l) => p.line_to(l.p1),
                    PathSeg::Quad(q) => p.quad_to(q.p1, q.p2),
                    PathSeg::Cubic(c) => p.curve_to(c.p1, c.p2, c.p3),
                }
            }
            p.close_path();
            p
        })
        .collect::<Vec<_>>();
    Ok(new_paths)
}

fn align(
    first: &[AnnotatedSegment],
    second: &[AnnotatedSegment],
    _glyph_name: &str,
    _round: usize,
) -> Result<(Vec<AnnotatedSegment>, Vec<AnnotatedSegment>), BabelfontError> {
    let sig1 = signatures(first);
    let sig2 = signatures(second);
    let target = scs_soft(&sig1, &sig2);

    let new_first = augment_to_target(first, &target)?;
    let new_second = augment_to_target(second, &target)?;
    Ok((new_first, new_second))
}

/// A filter that makes interpolatable paths structurally compatible by inserting nodes.
#[derive(Default)]
pub struct MakeCompatible;

impl MakeCompatible {
    /// Create a new MakeCompatible filter
    pub fn new() -> Self {
        MakeCompatible
    }
}

impl FontFilter for MakeCompatible {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let default_master = font
            .default_master()
            .ok_or(BabelfontError::NoDefaultMaster)?;
        let default_master_id = default_master.id.clone();

        for glyph in font.glyphs.iter_mut() {
            let interpolatable = glyph
                .layers
                .iter()
                .enumerate()
                .filter(|(_ix, l)| l.should_interpolate())
                .map(|(ix, _)| ix)
                .collect::<Vec<_>>();
            if interpolatable.len() < 2 {
                continue;
            }

            let default_layer_ix = interpolatable.iter().copied().find(|ix| {
                glyph.layers[*ix].master == LayerType::DefaultForMaster(default_master_id.clone())
            });
            let Some(default_layer_ix) = default_layer_ix else {
                log::warn!(
                    "makecompatible: glyph '{}' has no layer for default master '{}', skipping",
                    glyph.name,
                    default_master_id
                );
                continue;
            };

            // Closed paths only. Rotate starts to bottom-left-most on-curve.
            for &layer_ix in &interpolatable {
                let path_ixs = path_shape_indices(&glyph.layers[layer_ix]);
                for path_shape_ix in path_ixs {
                    if let Some(path) = get_path_mut(glyph, layer_ix, path_shape_ix) {
                        if !path.closed {
                            log::warn!(
                                "makecompatible: glyph '{}' has open path in interpolatable layer; skipping glyph",
                                glyph.name
                            );
                            continue;
                        }
                        rotate_closed_path_to_bottom_left(path);
                    }
                }
            }

            let path_ixs_by_layer = interpolatable
                .iter()
                .map(|ix| (*ix, path_shape_indices(&glyph.layers[*ix])))
                .collect::<Vec<_>>();
            let default_count = path_ixs_by_layer
                .iter()
                .find(|(ix, _)| *ix == default_layer_ix)
                .map(|(_, p)| p.len())
                .unwrap_or(0);
            if path_ixs_by_layer
                .iter()
                .any(|(_, p)| p.len() != default_count)
            {
                log::warn!(
                    "makecompatible: glyph '{}' has differing path counts across interpolatable layers; skipping",
                    glyph.name
                );
                continue;
            }
            for i in 0..default_count {
                let mut ordered_path_ixs = path_ixs_by_layer.clone();
                ordered_path_ixs.sort_by_key(
                    |(layer_ix, _)| {
                        if *layer_ix == default_layer_ix {
                            0
                        } else {
                            1
                        }
                    },
                );

                // Get all paths across layers
                let correlated_paths = ordered_path_ixs
                    .iter()
                    .map(|(layer_ix, path_ixs)| {
                        let path_ix = path_ixs[i];
                        #[allow(clippy::unwrap_used)]
                        glyph.layers[*layer_ix].shapes[path_ix]
                            .as_path()
                            .unwrap()
                            .to_kurbo()
                    })
                    .collect::<Result<Vec<_>, BabelfontError>>()?;
                // If the paths have different signum areas, skip for now. They need direction correction
                // before we can make them compatible, and that's a bit more work.
                let areas = correlated_paths
                    .iter()
                    .map(|p| p.area())
                    .collect::<Vec<_>>();
                if areas.iter().any(|a| a.signum() != areas[0].signum()) {
                    log::warn!(
                        "makecompatible: glyph '{}' has paths with different winding directions across interpolatable layers; skipping",
                        glyph.name
                    );
                    continue;
                }
                let new_paths = make_all_compatible(&correlated_paths, &glyph.name)?;
                // Put them back
                for (j, (layer_ix, path_ixs)) in ordered_path_ixs.iter().enumerate() {
                    let new_path = Path::from(new_paths[j].clone());
                    let path_ix = path_ixs[i];
                    if let Some(path) = get_path_mut(glyph, *layer_ix, path_ix) {
                        *path = new_path;
                    }
                }
            }
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(MakeCompatible::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("makecompatible")
            .long("make-compatible")
            .help("Make interpolatable paths structurally compatible by inserting nodes")
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;

    fn assert_sigs_are_consistent(glyph_name: &str, path_a: &Path, path_b: &Path) -> bool {
        let bez_a = path_a.to_kurbo().unwrap();
        let bez_b = path_b.to_kurbo().unwrap();
        let annotated_a = annotate_segments(bez_a.segments().collect());
        let annotated_b = annotate_segments(bez_b.segments().collect());
        let sig_a = signatures(&annotated_a);
        let sig_b = signatures(&annotated_b);
        if sig_a.len() != sig_b.len() {
            panic!(
                "Inconsistent signatures for glyph '{}': different segment counts ({} vs {}):\nA={:?}\nB={:?}",
                glyph_name,
                sig_a.len(),
                sig_b.len(),
                sig_a,
                sig_b
            );
        }
        for (ix, (siga, sigb)) in sig_a.iter().zip(sig_b.iter()).enumerate() {
            if !soft_signature_match(*siga, *sigb) {
                panic!(
                    "Inconsistent signatures for glyph '{}', segment {}: {:?} vs {:?}\nA={:?}\nB={:?}",
                    glyph_name, ix, siga, sigb, sig_a, sig_b
                );
            }
        }
        true
    }

    #[test]
    fn test_integration() {
        let path = "resources/Tirra.babelfont";
        let mut font = crate::load(path).unwrap();
        MakeCompatible::new().apply(&mut font).unwrap();

        let glyph = font.glyphs.get("f").unwrap();
        // Check for full signature match
        let path1 = glyph.layers[0].paths().last().unwrap();
        let path2 = glyph.layers[1].paths().last().unwrap();
        assert_eq!(path1.nodes.len(), path2.nodes.len());
        assert!(path1
            .nodes
            .iter()
            .zip(path2.nodes.iter())
            .all(|(n1, n2)| n1.nodetype == n2.nodetype));

        // Test some easy cases worked.
        for glyph_name in ["a", "m", "n", "f", "b", "d", "p"].iter() {
            let glyph = font.glyphs.get(glyph_name).unwrap();
            let g1_path_iter = glyph.layers[0].paths();
            let g2_path_iter = glyph.layers[1].paths();
            for (ix, (p1, p2)) in g1_path_iter.zip(g2_path_iter).enumerate() {
                assert_sigs_are_consistent(&format!("{} path {}", glyph_name, ix), p1, p2);
            }
        }
        // Harder cases - wrap around the start point
        let glyph = font.glyphs.get("U").unwrap();
        let outer0 = glyph.layers[0].paths().next().unwrap();
        let outer1 = glyph.layers[1].paths().next().unwrap();
        assert_sigs_are_consistent("U outer", outer0, outer1);
        let glyph = font.glyphs.get("l").unwrap();
        let outer0 = glyph.layers[0].paths().next().unwrap();
        let outer1 = glyph.layers[1].paths().next().unwrap();
        assert_sigs_are_consistent("l outer", outer0, outer1);
    }
}
