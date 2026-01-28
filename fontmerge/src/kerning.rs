use std::collections::{HashMap, HashSet};

use babelfont::{BabelfontError, Font, SmolStr};
use fontdrasil::{
    coords::{Location, NormalizedSpace},
    variations::VariationModel,
};
use indexmap::IndexMap;

use crate::{
    designspace::{fontdrasil_axes, Strategy},
    error::FontmergeError,
    glyphset::GlyphsetFilter,
};

// Remove glyphs to be imported from the target's kerning groups, so that
// importing the source kerning then does not lead to duplicate group
// membership if their membership changed. Return a vec of group names to clean.
fn clean_groups(
    f1_groups: &mut IndexMap<SmolStr, Vec<SmolStr>>,
    f2_groups: &IndexMap<SmolStr, Vec<SmolStr>>,
    glyphset_filter: &GlyphsetFilter,
) -> HashSet<SmolStr> {
    let mut kerning_groups_to_be_cleaned = HashSet::new();
    for (group_name, group) in f1_groups {
        // If the exact same group exists in font2, we're fine, ignore
        if f2_groups
            .get(group_name)
            .unwrap_or(&vec![])
            .iter()
            .collect::<HashSet<_>>()
            == group.iter().collect::<HashSet<_>>()
        {
            continue;
        }
        let new_members = group
            .iter()
            .filter(|glyph_name| !glyphset_filter.incoming_glyphset.contains(*glyph_name))
            .cloned()
            .collect::<Vec<_>>();
        if new_members.is_empty() {
            kerning_groups_to_be_cleaned.insert(group_name.clone());
        } else {
            *group = new_members;
        }
    }
    kerning_groups_to_be_cleaned
}

fn get_kerning_table_for_master(
    font: &Font,
    strategy: &Strategy,
) -> Result<IndexMap<(SmolStr, SmolStr), i16>, BabelfontError> {
    match strategy {
        Strategy::Exact {
            layer,
            master_name: _,
            clamped: _,
        } => Ok(font
            .masters
            .iter()
            .find(|m| &m.id == layer)
            .map(|m| m.kerning.clone())
            .unwrap_or_default()),
        Strategy::InterpolateOrIntermediate {
            location,
            clamped: _,
        } => {
            // OK, this is going to be hellish.
            #[allow(clippy::unwrap_used)] // We did this several times already by this point
            let axes = fontdrasil_axes(&font.axes).unwrap();
            log::debug!("Interpolating kerning at location {:?}", location);
            let target_location = location.to_normalized(&axes);
            let non_sparse_masters = font
                .masters
                .iter()
                .filter(|m| !m.is_sparse(font))
                .collect::<Vec<_>>();
            let mut result = IndexMap::new();
            let locations_in_order = non_sparse_masters
                .iter()
                .map(|m| m.location.to_normalized(&axes))
                .collect::<Vec<Location<NormalizedSpace>>>();
            let locations: HashSet<Location<NormalizedSpace>> =
                locations_in_order.iter().cloned().collect();
            let model = VariationModel::new(locations, axes.axis_order());
            let all_keys: HashSet<(SmolStr, SmolStr)> = font
                .masters
                .iter()
                .flat_map(|m| m.kerning.keys().cloned())
                .collect();
            for key in all_keys {
                // Collect all the values for this key
                let mut kerns_positions: HashMap<Location<NormalizedSpace>, Vec<f64>> =
                    HashMap::new();
                for master in non_sparse_masters.iter() {
                    if let Some(&value) = master.kerning.get(&key) {
                        let loc = master.location.to_normalized(&axes);
                        kerns_positions.entry(loc).or_default().push(value as f64);
                    }
                    let kern_deltas = model.deltas(&kerns_positions)?;
                    let interpolated_kern =
                        model.interpolate_from_deltas(&target_location, &kern_deltas);
                    if let Some(interpolated_kern) = interpolated_kern.first() {
                        result.insert(key.clone(), *interpolated_kern as i16);
                    }
                }
            }
            Ok(result)
        }
        Strategy::Failed(_) => Ok(IndexMap::default()),
    }
}

pub(crate) fn merge_kerning(
    font1: &mut Font,
    font2: &mut Font,
    glyphset_filter: &GlyphsetFilter,
    strategies: &[Strategy],
) -> Result<(), FontmergeError> {
    log::info!("Merging kerning");
    for group in font2.first_kern_groups.values_mut() {
        group.retain(|glyph_name| glyphset_filter.incoming_glyphset.contains(glyph_name));
    }
    for group in font2.second_kern_groups.values_mut() {
        group.retain(|glyph_name| glyphset_filter.incoming_glyphset.contains(glyph_name));
    }
    // Drop empty groups
    font2.first_kern_groups.retain(|_, group| !group.is_empty());
    font2
        .second_kern_groups
        .retain(|_, group| !group.is_empty());
    let final_glyphset: HashSet<SmolStr> =
        glyphset_filter.final_glyphset().iter().cloned().collect();
    let first_groups_to_clean = clean_groups(
        &mut font1.first_kern_groups,
        &font2.first_kern_groups,
        glyphset_filter,
    );
    let second_groups_to_clean = clean_groups(
        &mut font1.second_kern_groups,
        &font2.second_kern_groups,
        glyphset_filter,
    );
    for (master, strategy) in font1.masters.iter_mut().zip(strategies.iter()) {
        log::debug!(
            "Merging kerning for master '{}', strategy: {}",
            master.name.get_default().unwrap_or(&master.id),
            strategy
        );
        let kern = &mut master.kerning;
        kern.retain(|(left, right), _| {
            !first_groups_to_clean.contains(left.trim_start_matches('@'))
                && !second_groups_to_clean.contains(right.trim_start_matches('@'))
        });
        let font2_kerntable = get_kerning_table_for_master(font2, strategy).map_err(|e| {
            FontmergeError::Interpolation(format!(
                "Failed to get kerning table for master '{}': {}",
                master.name.get_default().unwrap_or(&master.id),
                e
            ))
        })?;
        for ((first, second), value) in font2_kerntable {
            let mut first_glyphs = if first.starts_with('@') {
                font2
                    .first_kern_groups
                    .get(first.trim_start_matches('@'))
                    .cloned()
                    .unwrap_or_default()
            } else {
                vec![first.clone()]
            };
            first_glyphs.retain(|g| final_glyphset.contains(g));
            let mut second_glyphs = if second.starts_with('@') {
                font2
                    .second_kern_groups
                    .get(second.trim_start_matches('@'))
                    .cloned()
                    .unwrap_or_default()
            } else {
                vec![second.clone()]
            };
            second_glyphs.retain(|g| final_glyphset.contains(g));
            if first_glyphs.is_empty() || second_glyphs.is_empty() {
                continue;
            }

            // Check the groups exist
            if let Some(groupname) = first.strip_prefix("@") {
                if let Some(group) = font1.first_kern_groups.get_mut(groupname) {
                    group.extend(first_glyphs);
                    // Deduplicate
                    group.sort();
                    group.dedup();
                } else {
                    font1
                        .first_kern_groups
                        .insert(groupname.into(), first_glyphs);
                }
            }
            if let Some(groupname) = second.strip_prefix("@") {
                if let Some(group) = font1.second_kern_groups.get_mut(groupname) {
                    group.extend(second_glyphs);
                    // Deduplicate
                    group.sort();
                    group.dedup();
                } else {
                    font1
                        .second_kern_groups
                        .insert(second[1..].into(), second_glyphs);
                }
            }

            // Just add the kern
            kern.insert((first, second), value);
        }
    }
    log::debug!("Merged kerning tables");
    Ok(())
}
