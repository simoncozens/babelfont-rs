use std::collections::HashSet;

use crate::{
    filters::FontFilter, interpolate::interpolate_layer, BabelfontError, Font, Glyph, Layer,
    LayerType, Shape,
};
use fontdrasil::{
    coords::{DesignSpace, Location},
    types::Axes,
};

const EPSILON: f64 = 1e-6;

/// Remove low-contribution interpolatable layers and prune now-unused non-corner masters.
pub struct RemoveExtraneousLayers {
    threshold: f64,
}

impl RemoveExtraneousLayers {
    /// Create a new RemoveExtraneousLayers filter.
    pub fn new(threshold: f64) -> Self {
        Self { threshold }
    }
}

impl FontFilter for RemoveExtraneousLayers {
    fn apply(&self, font: &mut Font) -> Result<(), BabelfontError> {
        let (corner_master_ids, non_corner_master_ids) = classify_masters(font)?;
        if non_corner_master_ids.is_empty() {
            log::info!("removeextraneouslayers: no non-corner masters found");
            return Ok(());
        }

        let axes = font.fontdrasil_axes()?;

        // Layer::effective_location only needs master locations, so keep a lightweight lookup font.
        let mut location_font = Font::new();
        location_font.masters = font.masters.clone();

        for glyph in font.glyphs.iter_mut() {
            if !glyph.compatibility_errors().is_empty() {
                log::info!(
                    "removeextraneouslayers: skipping glyph '{}' due to compatibility errors: {:?}",
                    glyph.name,
                    glyph.compatibility_errors()
                );
                continue;
            }
            process_glyph(
                glyph,
                &location_font,
                &axes,
                &corner_master_ids,
                self.threshold,
            )?;
        }

        cleanup_empty_non_corner_masters(font, &non_corner_master_ids);
        Ok(())
    }

    fn from_str(s: &str) -> Result<Self, BabelfontError>
    where
        Self: Sized,
    {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(BabelfontError::FilterError(
                "removeextraneouslayers requires a numeric threshold".to_string(),
            ));
        }
        let threshold = trimmed.parse::<f64>().map_err(|_| {
            BabelfontError::FilterError(format!(
                "Invalid removeextraneouslayers threshold: {}",
                trimmed
            ))
        })?;
        if !threshold.is_finite() {
            return Err(BabelfontError::FilterError(
                "removeextraneouslayers threshold must be finite".to_string(),
            ));
        }
        Ok(Self::new(threshold))
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("removeextraneouslayers")
            .long("remove-extraneous-layers")
            .help(
                "Remove low-contribution interpolatable layers and prune empty non-corner masters",
            )
            .value_name("THRESHOLD")
            .action(clap::ArgAction::Append)
    }
}

fn classify_masters(font: &Font) -> Result<(HashSet<String>, HashSet<String>), BabelfontError> {
    let mut corner_master_ids = HashSet::new();
    let mut non_corner_master_ids = HashSet::new();

    for master in &font.masters {
        let mut is_corner = false;
        for axis in &font.axes {
            let Some((min_u, default_u, max_u)) = axis.bounds() else {
                continue;
            };
            let min_d = axis.userspace_to_designspace(min_u)?.to_f64();
            let default_d = axis.userspace_to_designspace(default_u)?.to_f64();
            let max_d = axis.userspace_to_designspace(max_u)?.to_f64();
            let master_value = master
                .location
                .get(axis.tag)
                .map(|x| x.to_f64())
                .unwrap_or(default_d);

            if approx_eq(master_value, min_d)
                || approx_eq(master_value, default_d)
                || approx_eq(master_value, max_d)
            {
                is_corner = true;
                break;
            }
        }

        if is_corner {
            corner_master_ids.insert(master.id.clone());
        } else {
            non_corner_master_ids.insert(master.id.clone());
        }
    }

    log::debug!(
        "removeextraneouslayers: classified {} corner master(s) and {} non-corner master(s)",
        corner_master_ids.len(),
        non_corner_master_ids.len()
    );

    Ok((corner_master_ids, non_corner_master_ids))
}

fn process_glyph(
    glyph: &mut Glyph,
    location_font: &Font,
    axes: &Axes,
    corner_master_ids: &HashSet<String>,
    threshold: f64,
) -> Result<(), BabelfontError> {
    loop {
        let candidates = removable_layer_indices(glyph, corner_master_ids);
        if candidates.is_empty() {
            log::info!(
                "removeextraneouslayers: no more removable layers for glyph '{}'",
                glyph.name
            );
            break;
        }

        let mut best: Option<(usize, f64)> = None;
        for candidate_ix in candidates {
            let contribution = contribution_for_layer(glyph, candidate_ix, location_font, axes)?;
            log::info!(
                "removeextraneouslayers: glyph '{}', candidate layer {} has contribution {}",
                glyph.name,
                candidate_ix,
                contribution
            );
            if !contribution.is_finite() {
                continue;
            }

            match best {
                None => best = Some((candidate_ix, contribution)),
                Some((_, best_value)) if contribution < best_value => {
                    best = Some((candidate_ix, contribution));
                }
                _ => {}
            }
        }

        let Some((best_ix, best_contribution)) = best else {
            break;
        };

        if best_contribution >= threshold {
            break;
        }

        glyph.layers.remove(best_ix);
    }

    Ok(())
}

fn removable_layer_indices(glyph: &Glyph, corner_master_ids: &HashSet<String>) -> Vec<usize> {
    glyph
        .layers
        .iter()
        .enumerate()
        .filter_map(|(ix, layer)| {
            if !layer.should_interpolate() {
                return None;
            }
            if let LayerType::DefaultForMaster(master_id) = &layer.master {
                if corner_master_ids.contains(master_id) {
                    return None;
                }
            }
            Some(ix)
        })
        .collect()
}

fn contribution_for_layer(
    glyph: &Glyph,
    candidate_ix: usize,
    location_font: &Font,
    axes: &Axes,
) -> Result<f64, BabelfontError> {
    let Some(candidate_layer) = glyph.layers.get(candidate_ix) else {
        return Ok(f64::INFINITY);
    };

    let Some(target_location) = candidate_layer.effective_location(location_font) else {
        return Ok(f64::INFINITY);
    };
    let normalized_target = target_location.to_normalized(axes)?;

    let layers_locations: Vec<(Location<DesignSpace>, &Layer)> = glyph
        .layers
        .iter()
        .enumerate()
        .filter(|(ix, layer)| *ix != candidate_ix && layer.should_interpolate())
        .filter_map(|(_, layer)| {
            layer
                .effective_location(location_font)
                .map(|loc| (loc, layer))
        })
        .collect();

    if layers_locations.is_empty() {
        return Ok(f64::INFINITY);
    }

    let interpolated = match interpolate_layer(
        glyph.name.as_str(),
        &layers_locations,
        axes,
        &normalized_target,
    ) {
        Ok(layer) => layer,
        Err(_) => return Ok(f64::INFINITY),
    };

    Ok(layer_difference_score(candidate_layer, &interpolated))
}

fn layer_difference_score(candidate: &Layer, interpolated: &Layer) -> f64 {
    let mut score = (candidate.width - interpolated.width).abs() as f64;

    for (a, b) in candidate.shapes.iter().zip(interpolated.shapes.iter()) {
        score += shape_difference_score(a, b);
    }
    score
}

fn shape_difference_score(a: &Shape, b: &Shape) -> f64 {
    match (a, b) {
        (Shape::Path(path_a), Shape::Path(path_b)) => path_difference_score(path_a, path_b),
        (Shape::Component(comp_a), Shape::Component(comp_b)) => {
            let mut score = 0.0;
            if comp_a.reference != comp_b.reference {
                unreachable!("component reference mismatch should be caught in compatibility checks before filtering");
            }
            let a_coeffs = comp_a.transform.as_affine().as_coeffs();
            let b_coeffs = comp_b.transform.as_affine().as_coeffs();
            for (x, y) in a_coeffs.iter().zip(b_coeffs.iter()) {
                score += (x - y).abs();
            }
            score
        }
        _ => unreachable!("shape types should be checked before calling shape_difference_score"),
    }
}

fn path_difference_score(path_a: &crate::Path, path_b: &crate::Path) -> f64 {
    let mut score = 0.0;
    for (node_a, node_b) in path_a.nodes.iter().zip(path_b.nodes.iter()) {
        score += (node_a.x - node_b.x).hypot(node_a.y - node_b.y);
    }

    score
}

fn cleanup_empty_non_corner_masters(font: &mut Font, non_corner_master_ids: &HashSet<String>) {
    let mut used_master_ids = HashSet::new();
    for glyph in font.glyphs.iter() {
        for layer in &glyph.layers {
            match &layer.master {
                LayerType::DefaultForMaster(master_id)
                | LayerType::AssociatedWithMaster(master_id)
                    if non_corner_master_ids.contains(master_id) =>
                {
                    used_master_ids.insert(master_id.clone());
                }
                _ => {}
            }
        }
    }

    let before = font.masters.len();
    font.masters.retain(|master| {
        !non_corner_master_ids.contains(&master.id) || used_master_ids.contains(&master.id)
    });
    let removed = before.saturating_sub(font.masters.len());
    if removed > 0 {
        log::info!(
            "removeextraneouslayers: removed {} non-corner master(s) with no remaining layers",
            removed
        );
    }
}

fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPSILON
}
