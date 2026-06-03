use std::collections::{HashMap, HashSet};
use unicode_normalization::UnicodeNormalization;

use kurbo::{Affine, Point, Shape};
use smol_str::SmolStr;

use crate::{filters::FontFilter, Component, Layer, LayerType, Path, Shape as BbShape};

#[derive(Default, Debug)]
pub struct RecomposeEncoded {}

const GEOMETRY_EPSILON: f64 = 0.01;

#[derive(Debug, Clone)]
struct ComponentMatch {
    component: Component,
    consumed_path_indices: Vec<usize>,
}

#[derive(Debug, Clone)]
struct LayerPlan {
    layer_index: usize,
    components: Vec<Component>,
    remaining_paths: Vec<Path>,
    references: Vec<SmolStr>,
}

fn shape_to_area_bits(path: &Path) -> Option<u64> {
    Some(path.to_kurbo().ok()?.area().abs().to_bits())
}

fn affine_is_invertible(transform: Affine) -> bool {
    let [a, b, c, d, _, _] = transform.as_coeffs();
    (a * d - b * c).abs() > 1e-9
}

fn point_distance(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

fn transformed_path_matches(source: &Path, target: &Path, transform: Affine) -> bool {
    if source.closed != target.closed || source.nodes.len() != target.nodes.len() {
        return false;
    }
    if source.nodes.is_empty() {
        return true;
    }

    if !source.closed {
        return source
            .nodes
            .iter()
            .zip(target.nodes.iter())
            .all(|(src, tgt)| {
                src.nodetype == tgt.nodetype
                    && point_distance(
                        transform * Point::new(src.x, src.y),
                        Point::new(tgt.x, tgt.y),
                    ) <= GEOMETRY_EPSILON
            });
    }

    let len = source.nodes.len();
    if (0..len).any(|rotation| {
        source.nodes.iter().enumerate().all(|(ix, src)| {
            let target_ix = (ix + rotation) % len;
            let tgt = &target.nodes[target_ix];
            src.nodetype == tgt.nodetype
                && point_distance(
                    transform * Point::new(src.x, src.y),
                    Point::new(tgt.x, tgt.y),
                ) <= GEOMETRY_EPSILON
        })
    }) {
        return true;
    }

    let mut used = vec![false; target.nodes.len()];
    for src in &source.nodes {
        let src_point = transform * Point::new(src.x, src.y);
        let mut matched = false;
        for (ix, tgt) in target.nodes.iter().enumerate() {
            if used[ix] || tgt.nodetype != src.nodetype {
                continue;
            }
            let tgt_point = Point::new(tgt.x, tgt.y);
            if point_distance(src_point, tgt_point) <= GEOMETRY_EPSILON {
                used[ix] = true;
                matched = true;
                break;
            }
        }
        if !matched {
            return false;
        }
    }
    true
}

fn translation_for_correspondence(source: &Path, target: &Path, rotation: usize) -> Option<Affine> {
    if source.closed != target.closed || source.nodes.len() != target.nodes.len() {
        return None;
    }
    if source.nodes.is_empty() {
        return Some(Affine::IDENTITY);
    }
    let len = source.nodes.len();
    if !source.closed && rotation != 0 {
        return None;
    }

    let source_anchor = &source.nodes[0];
    let target_anchor = &target.nodes[rotation % len];
    if source_anchor.nodetype != target_anchor.nodetype {
        return None;
    }
    let dx = target_anchor.x - source_anchor.x;
    let dy = target_anchor.y - source_anchor.y;
    let transform = Affine::translate((dx, dy));
    transformed_path_matches(source, target, transform).then_some(transform)
}

fn pick_affine_from_correspondence(source: &Path, target: &Path) -> Option<Affine> {
    if source.nodes.len() != target.nodes.len() {
        return None;
    }
    let len = source.nodes.len();
    let rotations: Vec<usize> = if source.closed {
        (0..len).collect()
    } else {
        vec![0]
    };

    for rotation in &rotations {
        if let Some(transform) = translation_for_correspondence(source, target, *rotation) {
            return Some(transform);
        }
    }

    if source.nodes.len() < 3 {
        return None;
    }

    for rotation in rotations {
        let mut source_points = Vec::with_capacity(len);
        let mut target_points = Vec::with_capacity(len);

        let mut node_types_match = true;
        for (ix, src) in source.nodes.iter().enumerate() {
            let tgt = &target.nodes[(ix + rotation) % len];
            if src.nodetype != tgt.nodetype {
                node_types_match = false;
                break;
            }
            source_points.push(Point::new(src.x, src.y));
            target_points.push(Point::new(tgt.x, tgt.y));
        }
        if !node_types_match {
            continue;
        }

        for i in 0..len {
            for j in (i + 1)..len {
                for k in (j + 1)..len {
                    let p0 = source_points[i];
                    let p1 = source_points[j];
                    let p2 = source_points[k];
                    let q0 = target_points[i];
                    let q1 = target_points[j];
                    let q2 = target_points[k];

                    // Build an affine transform from three point correspondences.
                    let src_basis = Affine::new([
                        p1.x - p0.x,
                        p1.y - p0.y,
                        p2.x - p0.x,
                        p2.y - p0.y,
                        p0.x,
                        p0.y,
                    ]);
                    if !affine_is_invertible(src_basis) {
                        continue;
                    }
                    let tgt_basis = Affine::new([
                        q1.x - q0.x,
                        q1.y - q0.y,
                        q2.x - q0.x,
                        q2.y - q0.y,
                        q0.x,
                        q0.y,
                    ]);
                    let transform = tgt_basis * src_basis.inverse();

                    if transformed_path_matches(source, target, transform) {
                        return Some(transform);
                    }
                }
            }
        }
    }
    None
}

fn assign_candidate_paths(
    candidate_paths: &[Path],
    target_paths: &[Path],
    transform: Affine,
) -> Option<Vec<usize>> {
    let mut options: Vec<Vec<usize>> = candidate_paths
        .iter()
        .map(|candidate_path| {
            target_paths
                .iter()
                .enumerate()
                .filter_map(|(ix, target_path)| {
                    transformed_path_matches(candidate_path, target_path, transform).then_some(ix)
                })
                .collect()
        })
        .collect();

    if options.iter().any(Vec::is_empty) {
        return None;
    }

    // Deterministic order helps reproducibility when multiple solutions exist.
    for opts in &mut options {
        opts.sort_unstable();
    }

    let mut order: Vec<usize> = (0..candidate_paths.len()).collect();
    order.sort_by_key(|ix| options[*ix].len());

    fn backtrack(
        depth: usize,
        order: &[usize],
        options: &[Vec<usize>],
        used_targets: &mut HashSet<usize>,
        assignments: &mut [usize],
    ) -> bool {
        if depth == order.len() {
            return true;
        }
        let source_ix = order[depth];
        for target_ix in &options[source_ix] {
            if used_targets.contains(target_ix) {
                continue;
            }
            used_targets.insert(*target_ix);
            assignments[source_ix] = *target_ix;
            if backtrack(depth + 1, order, options, used_targets, assignments) {
                return true;
            }
            used_targets.remove(target_ix);
        }
        false
    }

    let mut used_targets = HashSet::default();
    let mut assignments = vec![usize::MAX; candidate_paths.len()];
    if backtrack(0, &order, &options, &mut used_targets, &mut assignments) {
        Some(assignments)
    } else {
        None
    }
}

fn recompose_once(
    our_paths: &[Path],
    candidate_layers: &HashMap<SmolStr, Layer>,
    path_cache: &HashMap<SmolStr, HashSet<u64>>,
) -> (Vec<Component>, Vec<Path>) {
    if our_paths.is_empty() {
        return (vec![], vec![]);
    }

    let our_pathset = our_paths
        .iter()
        .flat_map(shape_to_area_bits)
        .collect::<HashSet<u64>>();

    let mut best_match: Option<ComponentMatch> = None;

    for (glyph_name, candidate_layer) in candidate_layers {
        let area_prefilter_passed = path_cache
            .get(glyph_name)
            .map(|candidate_pathset| candidate_pathset.is_subset(&our_pathset))
            .unwrap_or(false);

        let candidate_paths = candidate_layer.paths().cloned().collect::<Vec<_>>();
        if candidate_paths.is_empty() || candidate_paths.len() > our_paths.len() {
            continue;
        }

        let mut matched: Option<ComponentMatch> = None;

        'affine_search: for candidate_anchor in &candidate_paths {
            for target_anchor in our_paths {
                let Some(transform) =
                    pick_affine_from_correspondence(candidate_anchor, target_anchor)
                else {
                    continue;
                };
                let Some(assignments) =
                    assign_candidate_paths(&candidate_paths, our_paths, transform)
                else {
                    continue;
                };

                let mut consumed_path_indices = assignments;
                consumed_path_indices.sort_unstable();
                matched = Some(ComponentMatch {
                    component: Component {
                        reference: glyph_name.clone(),
                        transform: transform.into(),
                        location: Default::default(),
                        format_specific: Default::default(),
                    },
                    consumed_path_indices,
                });
                break 'affine_search;
            }
        }

        let Some(candidate_match) = matched else {
            continue;
        };
        if !area_prefilter_passed {
            // We still accept geometry-proven matches when area bits differ,
            // but treat them as lower priority than exact area prefilter hits.
        }
        let candidate_size = candidate_match.consumed_path_indices.len();
        let replace = match &best_match {
            None => true,
            Some(existing) => {
                candidate_size > existing.consumed_path_indices.len()
                    || (candidate_size == existing.consumed_path_indices.len()
                        && candidate_match.component.reference < existing.component.reference)
            }
        };
        if replace {
            best_match = Some(candidate_match);
        }
    }

    if let Some(matched) = best_match {
        let consumed = matched
            .consumed_path_indices
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        let remaining_paths = our_paths
            .iter()
            .enumerate()
            .filter_map(|(ix, path)| (!consumed.contains(&ix)).then_some(path.clone()))
            .collect::<Vec<_>>();
        (vec![matched.component], remaining_paths)
    } else {
        (vec![], our_paths.to_vec())
    }
}

fn plan_for_layer(
    layer_paths: &[Path],
    decomposition: &str,
    decomposition_lookup: &HashMap<String, Vec<SmolStr>>,
    current_glyph_name: &SmolStr,
    layer_candidates: &HashMap<SmolStr, Layer>,
    path_cache: &HashMap<SmolStr, HashSet<u64>>,
) -> (Vec<Component>, Vec<Path>) {
    let decomp_chars: Vec<char> = decomposition.chars().collect();
    if decomp_chars.len() <= 1 {
        return (vec![], layer_paths.to_vec());
    }

    let mut remaining_paths = layer_paths.to_vec();
    let mut components = vec![];
    let mut cursor = decomp_chars.len();
    let mut first_step = true;

    while cursor > 0 {
        let start_min = if first_step { 1 } else { 0 };
        let mut matched_here = false;
        let mut starts = (start_min..cursor).collect::<Vec<_>>();

        for start in starts.drain(..) {
            let chunk = decomp_chars[start..cursor].iter().collect::<String>();
            let Some(candidate_names) = decomposition_lookup.get(&chunk) else {
                continue;
            };

            let candidate_layers = candidate_names
                .iter()
                .filter(|name| *name != current_glyph_name)
                .filter_map(|name| {
                    layer_candidates
                        .get(name)
                        .cloned()
                        .map(|layer| (name.clone(), layer))
                })
                .collect::<HashMap<_, _>>();
            if candidate_layers.is_empty() {
                continue;
            }

            let (found_components, leftovers) =
                recompose_once(&remaining_paths, &candidate_layers, path_cache);
            if let Some(component) = found_components.first().cloned() {
                components.push(component);
                remaining_paths = leftovers;
                cursor = start;
                matched_here = true;
                break;
            }
        }

        if !matched_here {
            cursor -= 1;
        }
        first_step = false;
    }

    components.reverse();
    (components, remaining_paths)
}

impl FontFilter for RecomposeEncoded {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        // First we identify all encoded glyphs in the font with more than one
        // character in their canonical decomposition.
        let mut encoded_glyphs = font
            .glyphs
            .iter()
            .filter(|g| !g.codepoints.is_empty())
            .flat_map(|g| g.codepoints.iter().map(|cp| (g.name.clone(), cp)))
            .flat_map(|(name, cp)| char::try_from(*cp).map(|cr| (name, cr)))
            .map(|(name, c)| (c.nfd().collect::<String>(), name))
            .filter(|(decomp, _glyph)| decomp.len() > 1)
            .collect::<Vec<_>>();

        let decomposition_lookup = font
            .glyphs
            .iter()
            .filter(|g| !g.codepoints.is_empty())
            .flat_map(|g| {
                g.codepoints.iter().filter_map(|cp| {
                    char::try_from(*cp)
                        .ok()
                        .map(|c| (c.nfd().collect::<String>(), g.name.clone()))
                })
            })
            .fold(
                HashMap::<String, Vec<SmolStr>>::default(),
                |mut acc, (decomp, name)| {
                    acc.entry(decomp).or_default().push(name);
                    acc
                },
            );

        // Sort
        encoded_glyphs.sort_by_key(|(decomp, _glyph)| decomp.len());

        // For each interpolatable layer in all glyphs, create a cache made up of
        // a set of the unsigned areas of the *decomposed* paths. (This is just an
        // optimization to help us find matching contours as a quick first pass before
        // checking strict contour equality.)
        // This will cope badly with mutiple free-floating layers per glyph
        // but we can deal with that later.
        // u64 because it's f64.to_bits(). Unsigned area in case their path directions differ.
        let mut path_cache: HashMap<LayerType, HashMap<SmolStr, HashSet<u64>>> = HashMap::default();
        let mut decomposed_layer_cache: HashMap<LayerType, HashMap<SmolStr, Layer>> =
            HashMap::default();

        for glyph in font.glyphs.iter() {
            for layer in glyph.layers.iter() {
                if !layer.should_interpolate() {
                    continue;
                }
                let decomposed = layer.decomposed(font);
                let path_areas = decomposed
                    .shapes
                    .iter()
                    .filter_map(|x| x.as_path())
                    .flat_map(shape_to_area_bits)
                    .collect::<HashSet<u64>>();
                path_cache
                    .entry(layer.master.clone())
                    .or_default()
                    .insert(glyph.name.clone(), path_areas);
                decomposed_layer_cache
                    .entry(layer.master.clone())
                    .or_default()
                    .insert(glyph.name.clone(), decomposed);
            }
        }

        // Iterate over our list of encoded glyphs, shortest decomposition to longest.
        // For each glyph, look at each interpolatable layer, and see if we can match the paths in
        // that layer with the paths in the encoded glyphs for the Unicode
        // decompositions. We do this greedily from the *end*. ie. for codepoint
        // ṩ we fully decompose it to s, dotbelow, dotabove. We try to find a composed
        // glyph which matches the paths of dotbelow + dotabove. Hopefully we
        // don't find one (it would be a weird combination) so we look for one matching
        // just dotabove. Supposing we find a dotabove which matches an equivalent path in all
        // layers, we next look for a single glyph matching s + dotbelow.
        // Since we are traversing in order of length, if there's a sdotbelow glyph
        // in the font we will already have processed it. (This is why our matching
        // algorithm needs to compare decomposed paths.) Assuming all the paths
        // match up across layers, we can now replace the shapes of
        // ṩ with two components, dotabove and sdotbelow, with appropriate transformation
        // matrices.
        for (decomposition, glyph_name) in encoded_glyphs {
            let Some(glyph) = font.glyphs.get(glyph_name.as_str()) else {
                continue;
            };

            let mut plans = vec![];
            let mut all_layers_possible = true;
            for (layer_index, layer) in glyph.layers.iter().enumerate() {
                if !layer.should_interpolate() {
                    continue;
                }
                let decomposed_layer = layer.decomposed(font);
                let layer_paths = decomposed_layer.paths().cloned().collect::<Vec<_>>();
                if layer_paths.is_empty() {
                    continue;
                }

                let Some(path_cache_for_layer) = path_cache.get(&layer.master) else {
                    all_layers_possible = false;
                    break;
                };
                let Some(layer_candidates) = decomposed_layer_cache.get(&layer.master) else {
                    all_layers_possible = false;
                    break;
                };

                let (components, remaining_paths) = plan_for_layer(
                    &layer_paths,
                    &decomposition,
                    &decomposition_lookup,
                    &glyph_name,
                    layer_candidates,
                    path_cache_for_layer,
                );
                if components.is_empty() {
                    all_layers_possible = false;
                    break;
                }

                let references = components
                    .iter()
                    .map(|c| c.reference.clone())
                    .collect::<Vec<_>>();
                plans.push(LayerPlan {
                    layer_index,
                    components,
                    remaining_paths,
                    references,
                });
            }

            if !all_layers_possible || plans.is_empty() {
                continue;
            }

            let first_references = plans[0].references.clone();
            if plans
                .iter()
                .skip(1)
                .any(|plan| plan.references != first_references)
            {
                continue;
            }

            let Some(glyph_mut) = font.glyphs.get_mut(glyph_name.as_str()) else {
                continue;
            };
            for plan in plans {
                if let Some(layer_mut) = glyph_mut.layers.get_mut(plan.layer_index) {
                    let mut new_shapes = Vec::new();
                    new_shapes.extend(plan.components.into_iter().map(BbShape::Component));
                    new_shapes.extend(plan.remaining_paths.into_iter().map(BbShape::Path));
                    layer_mut.shapes = new_shapes;
                }
            }
        }
        Ok(())
    }

    fn from_str(s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        if s.is_empty() {
            Ok(Self::default())
        } else {
            Err(crate::BabelfontError::FilterError(format!(
                "recompose does not take an argument: {}",
                s
            )))
        }
    }
    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        unreachable!()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_recompose_encoded() {
        let mut font = crate::load("resources/test-recomposition.babelfont").unwrap();
        RecomposeEncoded::default().apply(&mut font).unwrap();
        let a_acute = font.glyphs.iter().find(|g| g.name == "Aacute").unwrap();
        for layer in a_acute.layers.iter().filter(|l| l.should_interpolate()) {
            let components = layer
                .shapes
                .iter()
                .filter_map(|s| s.as_component())
                .collect::<Vec<_>>();
            let path_count = layer.shapes.iter().filter_map(|s| s.as_path()).count();
            assert_eq!(
                components.len(),
                2,
                "Aacute layer {:?} refs={:?} path_count={}",
                layer.id,
                components
                    .iter()
                    .map(|c| c.reference.clone())
                    .collect::<Vec<_>>(),
                path_count
            );
            assert_eq!(path_count, 0);
        }
        let o_acute = font.glyphs.iter().find(|g| g.name == "Oacute").unwrap();
        for layer in o_acute.layers.iter().filter(|l| l.should_interpolate()) {
            let components = layer
                .shapes
                .iter()
                .filter_map(|s| s.as_component())
                .collect::<Vec<_>>();
            assert_eq!(
                components.len(),
                2,
                "Oacute layer {:?} refs={:?} path_count={}",
                layer.id,
                components
                    .iter()
                    .map(|c| c.reference.clone())
                    .collect::<Vec<_>>(),
                layer.shapes.iter().filter_map(|s| s.as_path()).count()
            );
            assert_eq!(components[0].reference, "O");
            assert_eq!(components[1].reference, "acutecomb");
        }
        let o_tildeacute = font
            .glyphs
            .iter()
            .find(|g| g.name == "Otildeacute")
            .unwrap();
        for layer in o_tildeacute
            .layers
            .iter()
            .filter(|l| l.should_interpolate())
        {
            let components = layer
                .shapes
                .iter()
                .filter_map(|s| s.as_component())
                .collect::<Vec<_>>();
            assert_eq!(
                components.len(),
                1,
                "Otildeacute layer {:?} refs={:?} path_count={}",
                layer.id,
                components
                    .iter()
                    .map(|c| c.reference.clone())
                    .collect::<Vec<_>>(),
                layer.shapes.iter().filter_map(|s| s.as_path()).count()
            );
            assert_eq!(components[0].reference, "O");
            assert_eq!(layer.shapes.iter().filter_map(|s| s.as_path()).count(), 2);
        }
    }
}
