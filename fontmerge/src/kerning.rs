use std::collections::{HashMap, HashSet};

use babelfont::Font;

use crate::{designspace::Strategy, glyphset::GlyphsetFilter};

// Remove glyphs to be imported from the target's kerning groups, so that
// importing the source kerning then does not lead to duplicate group
// membership if their membership changed. Return a vec of group names to clean.
fn clean_groups(
    f1_groups: &mut HashMap<String, Vec<String>>,
    f2_groups: &HashMap<String, Vec<String>>,
    glyphset_filter: &GlyphsetFilter,
) -> HashSet<String> {
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
) -> HashMap<(String, String), i16> {
    match strategy {
        Strategy::Exact {
            layer,
            master_name: _,
            clamped: _,
        } => font
            .masters
            .iter()
            .find(|m| &m.id == layer)
            .map(|m| m.kerning.clone())
            .unwrap_or_default(),
        Strategy::InterpolateOrIntermediate {
            location: _,
            clamped: _,
        } => {
            // Whether we need to put interpolated, intermediate kerning in kind of depends if you're
            // using fontmake or fontc to compile the eventual font. Fontmake requires a kerning table
            // for each master, and if it isn't present, treats it as zero. Fontc, on the other hand, will
            // just put whatever kerning information has available into the GPOS table, and if that's
            // "sparse", then it's fine and the interpolation will be done at layout time.

            // For now I am going to assume we're compiling with fontc, and return an empty table, simply
            // because interpolating kerning is a lot of work and I've got enough to do right now.
            log_once::warn_once!(
                "Interpolated or intermediate kerning not yet implemented; skipping kerning for this master. If you're compiling with fontmake, this may lead to missing kerning in the final font."
            );
            HashMap::new()
        }
        Strategy::Failed(_) => HashMap::default(),
    }
}

pub(crate) fn merge_kerning(
    font1: &mut Font,
    font2: &mut Font,
    glyphset_filter: &GlyphsetFilter,
    strategies: &[Strategy],
) {
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
    let final_glyphset: HashSet<String> =
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
        let font2_kerntable = get_kerning_table_for_master(font2, strategy);
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
                        .insert(groupname.to_string(), first_glyphs);
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
                        .insert(second[1..].to_string(), second_glyphs);
                }
            }

            // Just add the kern
            kern.insert((first, second), value);
        }
    }
    log::debug!("Merged kerning tables");
}
