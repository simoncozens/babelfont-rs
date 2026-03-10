use std::collections::{HashMap, HashSet};

use crate::{
    convertors::glyphs3::KEY_CUSTOM_PARAMETERS, features::PossiblyAutomaticCode, Axis,
    FormatSpecific, Layer, Tag,
};
use fea_rs_ast::{
    AsFea as _, GlyphContainer, GlyphName, SingleSubstStatement, Statement, VariationBlock,
};
use fontdrasil::coords::{DesignCoord, UserCoord};
use smol_str::SmolStr;

use crate::{convertors::glyphs3::KEY_ATTR, filters::FontFilter, LayerType};

#[derive(Default)]
/// A filter that converts Glyphs "bracket layers" to Feature Variation glyphs
pub struct GlyphsBracketLayers;

impl GlyphsBracketLayers {
    /// Create a new GlyphsBracketLayers filter
    pub fn new() -> Self {
        GlyphsBracketLayers
    }
}

#[derive(Hash, Eq, PartialEq, Debug, Clone, Copy)]
struct UserspaceMinMax {
    min: UserCoord,
    max: UserCoord,
}

// A Box is a dict mapping axis tags to (min, max) ranges
type Box = HashMap<Tag, UserspaceMinMax>;

// A hashable wrapper for Box, used as a key in HashMaps
#[derive(Eq, PartialEq, Debug, Clone)]
struct HashableBox(Box);

impl std::hash::Hash for HashableBox {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash the box by sorting all axes to ensure consistent ordering
        let mut sorted_axes: Vec<_> = self.0.iter().collect();
        sorted_axes.sort_by_key(|(tag, _)| *tag);
        for (tag, range) in sorted_axes {
            tag.hash(state);
            range.min.hash(state);
            range.max.hash(state);
        }
    }
}

// A DesignspaceRegion is a list of Boxes representing a more complex subset of the design space
#[derive(Eq, PartialEq, Debug, Clone)]
struct DesignspaceRegion(Vec<Box>);

impl std::hash::Hash for DesignspaceRegion {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash the region by sorting all boxes and their axes to ensure consistent ordering
        let mut sorted_boxes: Vec<_> = self.0.iter().collect();
        sorted_boxes.sort_by_key(|box_map| {
            let mut sorted_axes: Vec<_> = box_map.iter().collect();
            sorted_axes.sort_by_key(|(tag, _)| *tag);
            sorted_axes
                .iter()
                .map(|(tag, range)| (*tag, range.min, range.max))
                .collect::<Vec<_>>()
        });
        for box_map in sorted_boxes {
            let mut sorted_axes: Vec<_> = box_map.iter().collect();
            sorted_axes.sort_by_key(|(tag, _)| *tag);
            for (tag, range) in sorted_axes {
                tag.hash(state);
                range.min.hash(state);
                range.max.hash(state);
            }
        }
    }
}

impl DesignspaceRegion {
    fn from_rules_and_axes(
        rules: &[serde_json::Value],
        axes: &[Axis],
    ) -> Option<DesignspaceRegion> {
        let mut box_map = HashMap::new();
        #[allow(clippy::unwrap_used)]
        for (rule, axis) in rules.iter().filter_map(|v| v.as_object()).zip(axes.iter()) {
            // Fill out the box for this axis. If min or max is missing, use the full range of the axis.
            // Unwrapping axis limits is safe here, we checked below
            let min = rule
                .get("min")
                .and_then(|v| v.as_f64())
                .map(DesignCoord::new)
                .and_then(|d| axis.designspace_to_userspace(d).ok())
                .unwrap_or(axis.min.unwrap());
            let max = rule
                .get("max")
                .and_then(|v| v.as_f64())
                .map(DesignCoord::new)
                .and_then(|d| axis.designspace_to_userspace(d).ok())
                .unwrap_or(axis.max.unwrap());
            box_map.insert(axis.tag, UserspaceMinMax { min, max });
        }
        // A region is a list of boxes; this method creates a region with a single box
        Some(DesignspaceRegion(vec![box_map]))
    }

    fn to_name(&self) -> String {
        let mut name = String::new();
        for box_map in self.0.iter() {
            let mut parts = Vec::new();
            for (tag, range) in box_map.iter() {
                let part = format!(
                    "{}_{}_{}",
                    tag,
                    range
                        .min
                        .to_f64()
                        .to_string()
                        .replace("-", "m")
                        .replace(".", "p"),
                    range
                        .max
                        .to_f64()
                        .to_string()
                        .replace("-", "m")
                        .replace(".", "p")
                );
                parts.push(part);
            }
            name.push_str(&parts.join("_"));
        }
        name
    }
}

impl FontFilter for GlyphsBracketLayers {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let axes = font.axes.clone();
        // Ensure that axes have min and max. We can only convert if we know the full region of each axis.
        if axes.iter().any(|a| a.min.is_none() || a.max.is_none()) {
            log::warn!("Skipping Glyphs bracket layer conversion because at least one axis is missing min or max");
            return Ok(());
        }
        if font.features.prefixes.contains_key("Feature Variation") {
            log::warn!("Skipping Glyphs bracket layer conversion because font already has a 'Feature Variation' prefix");
            return Ok(());
        }
        let variation_feature = if let Some(feature) = font.format_specific.get(&format!(
            "{}Feature For Feature Variations",
            KEY_CUSTOM_PARAMETERS
        )) {
            feature.as_str().unwrap_or("rvrn")
        } else {
            "rvrn"
        };
        let mut all_ruleset_combinations: Vec<(DesignspaceRegion, SmolStr, SmolStr)> = Vec::new();
        let master_ids = font
            .masters
            .iter()
            .map(|m| m.id.clone())
            .collect::<Vec<_>>();
        let mut new_glyphs_to_add = Vec::new();
        for glyph in font.glyphs.iter_mut() {
            let mut bracket_layers_by_ruleset: HashMap<DesignspaceRegion, Vec<Layer>> =
                HashMap::new();
            let mut layer_ids_to_drop = HashSet::new();
            for layer in glyph.layers.iter() {
                if matches!(layer.master, LayerType::AssociatedWithMaster(_)) {
                    if let Some(axis_rules) = layer
                        .format_specific
                        .get(KEY_ATTR)
                        .and_then(|v| v.as_object())
                        .and_then(|v| v.get("axisRules"))
                        .and_then(|v| v.as_array())
                        .and_then(|v| DesignspaceRegion::from_rules_and_axes(v, &axes))
                    {
                        bracket_layers_by_ruleset
                            .entry(axis_rules)
                            .or_default()
                            .push(layer.clone());
                    }
                }
                if bracket_layers_by_ruleset.is_empty() {
                    continue;
                }
            }
            let mut counter = 1;
            for (ruleset, layers) in bracket_layers_by_ruleset {
                // Check we have one for each master
                if !master_ids.iter().all(|id| {
                    layers
                        .iter()
                        .any(|l| l.master == LayerType::AssociatedWithMaster(id.clone()))
                }) {
                    log::warn!("Skipping bracket layer for glyph {} ruleset {:?} because it doesn't have a layer for each master", glyph.name, ruleset);
                    continue;
                }
                // Create a new glyph for this ruleset
                let mut new_glyph = glyph.clone();
                new_glyph.name = format!("{}.VAR.{}", glyph.name, counter).into();
                counter += 1;
                // Remove these layers from the original glyph
                layer_ids_to_drop.extend(layers.iter().filter_map(|l| l.id.clone()));
                // Set each layer as the master
                new_glyph.layers = layers;
                for layer in new_glyph.layers.iter_mut() {
                    let LayerType::AssociatedWithMaster(current_assocation) = &layer.master else {
                        continue;
                    };
                    layer.master = LayerType::DefaultForMaster(current_assocation.clone());
                }
                new_glyph.codepoints.clear();
                all_ruleset_combinations.push((
                    ruleset,
                    glyph.name.clone(),
                    new_glyph.name.clone(),
                ));
                new_glyphs_to_add.push(new_glyph);
            }
            // Drop layers we moved
            glyph.layers.retain(|l| {
                if let Some(id) = &l.id {
                    !layer_ids_to_drop.contains(id)
                } else {
                    true
                }
            });
        }
        font.glyphs.extend(new_glyphs_to_add);

        // Split the boxes. Any overlapping rulesets must be split into distinct regions
        let split_rulesets = split_boxes(&all_ruleset_combinations);
        let mut fea = String::new();
        for (region, substs) in split_rulesets.iter() {
            let name = region.to_name();
            let condition = fea_rs_ast::ConditionSet::new(
                name.clone(),
                region
                    .0
                    .iter()
                    .flat_map(|box_map| {
                        box_map
                            .iter()
                            .map(|(tag, range)| {
                                let min = range.min.to_f64().round() as i16;
                                let max = range.max.to_f64().round() as i16;
                                (
                                    tag.to_string(),
                                    f32::from(min),
                                    f32::from(max),
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect(),
                0..0,
            );
            let substitutions = substs
                .iter()
                .map(|(from, to)| {
                    Statement::SingleSubst(SingleSubstStatement::new(
                        vec![GlyphContainer::GlyphName(GlyphName::new(from.as_str()))],
                        vec![GlyphContainer::GlyphName(GlyphName::new(to.as_str()))],
                        vec![],
                        vec![],
                        0..0,
                        false,
                    ))
                })
                .collect::<Vec<_>>();
            let feature_var = VariationBlock::new(
                variation_feature.into(),
                name,
                substitutions,
                false,
                0..0,
            );
            fea.push_str(&condition.as_fea(""));
            fea.push_str(&feature_var.as_fea(""));
            fea.push('\n');
        }
        font.features.prefixes.insert(
            "Feature Variation".into(),
            PossiblyAutomaticCode {
                code: fea,
                automatic: false,
                format_specific: FormatSpecific::default(),
            },
        );
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(GlyphsBracketLayers::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("glyphsbracketlayers")
            .long("convert-glyphs-bracket-layers")
            .help("Convert Glyphs bracket layers to Feature Variation glyphs")
            .action(clap::ArgAction::SetTrue)
    }
}

fn split_boxes(
    rulesets: &[(DesignspaceRegion, SmolStr, SmolStr)],
) -> HashMap<DesignspaceRegion, HashMap<SmolStr, SmolStr>> {
    // The box_map tracks which rulesets apply to each box in the design space
    // The value is a bitmask where bit i is set if ruleset i applies to this box
    let mut box_map: HashMap<HashableBox, u64> = HashMap::new();

    // Start with the entire design space represented by an empty box
    box_map.insert(HashableBox(HashMap::new()), 0);

    // For each ruleset, overlay all its boxes onto the existing box map
    for (rule_index, (region, _, _)) in rulesets.iter().enumerate() {
        let mut new_map: HashMap<HashableBox, u64> = HashMap::new();
        // Initialize new map with the entire space
        new_map.insert(HashableBox(HashMap::new()), 0);

        let curr_rank = 1u64 << rule_index;

        // For each box in the current region
        for curr_box in region.0.iter() {
            // For each existing box in the map, compute how it intersects with the current box
            for (existing_hbox, existing_rank) in box_map.iter() {
                let existing_box = &existing_hbox.0;
                let (intersection, remainder) = overlay_box(curr_box, existing_box);

                if let Some(inter_box) = intersection {
                    let rank = new_map.entry(HashableBox(inter_box)).or_insert(0);
                    *rank |= existing_rank | curr_rank;
                }

                if let Some(rem_box) = remainder {
                    let rank = new_map.entry(HashableBox(rem_box)).or_insert(0);
                    *rank |= existing_rank;
                }
            }
        }

        box_map = new_map;
    }

    // Generate output: convert box_map to substitution mappings
    let mut result = HashMap::new();

    // Sort boxes by rank (most specific first) - higher rank means more rulesets apply
    let mut sorted_boxes: Vec<_> = box_map.iter().collect();
    sorted_boxes.sort_by_key(|&(_, rank)| std::cmp::Reverse(*rank));

    for (hbox, rank) in sorted_boxes {
        // Skip boxes that don't have any ruleset applying to them
        if *rank == 0 {
            continue;
        }

        // Collect all substitutions that apply to this box
        let mut subst_map = HashMap::new();
        for (bit_index, (_, orig_glyph, subst_glyph)) in rulesets.iter().enumerate() {
            if (*rank & (1u64 << bit_index)) != 0 {
                subst_map.insert(orig_glyph.clone(), subst_glyph.clone());
            }
        }

        if !subst_map.is_empty() {
            // Wrap the box in a region (a region is a list of boxes)
            let region = DesignspaceRegion(vec![hbox.0.clone()]);
            result.insert(region, subst_map);
        }
    }

    result
}

fn overlay_box(top: &Box, bot: &Box) -> (Option<Box>, Option<Box>) {
    // Compute the intersection of top and bot boxes
    let mut intersection = bot.clone();
    for (tag, top_range) in top.iter() {
        intersection.insert(*tag, *top_range);
    }

    // Clip the intersection to axes that appear in both boxes
    for (tag, bot_range) in bot.iter() {
        if let Some(&top_range) = top.get(tag) {
            let minimum = top_range.min.max(bot_range.min);
            let maximum = top_range.max.min(bot_range.max);

            // Check if they actually intersect
            if minimum >= maximum {
                return (None, Some(bot.clone()));
            }

            intersection.insert(
                *tag,
                UserspaceMinMax {
                    min: minimum,
                    max: maximum,
                },
            );
        }
    }

    // Compute the remainder: the part of bot not covered by the intersection
    let mut remainder = bot.clone();
    let mut extruding = false;
    let mut fully_inside = true;

    // Check if bot has axes that top doesn't have
    for tag in top.keys() {
        if !bot.contains_key(tag) {
            extruding = true;
            fully_inside = false;
            break;
        }
    }

    // Check each axis that appears in both boxes
    for (tag, bot_range) in bot.iter() {
        if !top.contains_key(tag) {
            continue; // Axis range lies fully within (bot is unbounded on this axis)
        }
        // Unwraps here are safe because we added them in the loop above

        #[allow(clippy::unwrap_used)]
        let min1 = intersection.get(tag).unwrap().min;
        #[allow(clippy::unwrap_used)]
        let max1 = intersection.get(tag).unwrap().max;
        let min2 = bot_range.min;
        let max2 = bot_range.max;

        // Check if bot's range lies fully within the intersection
        if min1 <= min2 && max2 <= max1 {
            continue;
        }

        // Bot's range doesn't fully lie within the intersection's range
        // If we've already found one axis with this property, we can't
        // represent the remainder as a single box
        if extruding {
            return (Some(intersection), Some(bot.clone()));
        }

        extruding = true;
        fully_inside = false;

        // Try to compute a remainder by cutting on this axis
        if min1 <= min2 {
            // Right side survives
            let new_minimum = max1.max(min2);
            remainder.insert(
                *tag,
                UserspaceMinMax {
                    min: new_minimum,
                    max: max2,
                },
            );
        } else if max2 <= max1 {
            // Left side survives
            let new_maximum = min1.min(max2);
            remainder.insert(
                *tag,
                UserspaceMinMax {
                    min: min2,
                    max: new_maximum,
                },
            );
        } else {
            // Remainder leaks out on both sides - can't represent as a single box
            return (Some(intersection), Some(bot.clone()));
        }
    }

    if fully_inside {
        // bot is fully within the intersection - no remainder
        (Some(intersection), None)
    } else {
        (Some(intersection), Some(remainder))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    fn coord(val: f32) -> UserCoord {
        UserCoord::new(val as f64)
    }

    fn tag_from_str(s: &str) -> Tag {
        Tag::new_checked(s.as_bytes()).expect("Invalid tag")
    }

    fn create_box(ranges: &[(&str, f32, f32)]) -> Box {
        let mut box_map = HashMap::new();
        for (tag_str, min, max) in ranges {
            box_map.insert(
                tag_from_str(tag_str),
                UserspaceMinMax {
                    min: coord(*min),
                    max: coord(*max),
                },
            );
        }
        box_map
    }

    fn create_region(ranges: &[(&str, f32, f32)]) -> DesignspaceRegion {
        DesignspaceRegion(vec![create_box(ranges)])
    }

    #[test]
    fn test_overlay_box_basic() {
        // Test from featureVars.py: test_overlayBox
        // top = {"opsz": (0.75, 1.0), "wght": (0.5, 1.0)}
        // bot = {"wght": (0.25, 1.0)}
        // intersection = {"opsz": (0.75, 1.0), "wght": (0.5, 1.0)}
        // remainder = {"wght": (0.25, 1.0)}

        let top = create_box(&[("opsz", 0.75, 1.0), ("wght", 0.5, 1.0)]);
        let bot = create_box(&[("wght", 0.25, 1.0)]);

        let (inter_opt, rem_opt) = overlay_box(&top, &bot);

        let inter = inter_opt.expect("Expected intersection");
        assert_eq!(inter.len(), 2);
        assert_eq!(inter[&tag_from_str("opsz")].min, coord(0.75));
        assert_eq!(inter[&tag_from_str("opsz")].max, coord(1.0));
        assert_eq!(inter[&tag_from_str("wght")].min, coord(0.5));
        assert_eq!(inter[&tag_from_str("wght")].max, coord(1.0));

        let rem = rem_opt.expect("Expected remainder");
        assert_eq!(rem.len(), 1);
        assert_eq!(rem[&tag_from_str("wght")].min, coord(0.25));
        assert_eq!(rem[&tag_from_str("wght")].max, coord(1.0));
    }

    #[test]
    fn test_overlay_box_no_intersection() {
        // Two boxes that don't intersect
        let top = create_box(&[("wght", 0.0, 0.2)]);
        let bot = create_box(&[("wght", 0.5, 1.0)]);

        let (inter_opt, rem_opt) = overlay_box(&top, &bot);
        assert!(inter_opt.is_none());
        assert!(rem_opt.is_some());
    }

    #[test]
    fn test_split_boxes_simple() {
        // Simple test: two non-overlapping rulesets
        // Ruleset 1: wght (0.5, 1.0) -> dollar -> dollar.rvrn
        // Ruleset 2: wdth (0.5, 1.0) -> cent -> cent.rvrn

        let region1 = create_region(&[("wght", 0.5, 1.0)]);
        let region2 = create_region(&[("wdth", 0.5, 1.0)]);

        let rulesets = vec![
            (region1, "dollar".into(), "dollar.rvrn".into()),
            (region2, "cent".into(), "cent.rvrn".into()),
        ];

        let result = split_boxes(&rulesets);

        // Should have 3 regions:
        // 1. Both wght and wdth (intersection) - both substs apply
        // 2. Only wdth - cent subst applies
        // 3. Only wght - dollar subst applies
        assert_eq!(result.len(), 3);

        // Check that each region has the expected substitutions
        let mut found_both = false;
        let mut found_wdth_only = false;
        let mut found_wght_only = false;

        for (region, substs) in result.iter() {
            assert!(!region.0.is_empty());
            let box_region = &region.0[0];

            if box_region.contains_key(&tag_from_str("wght"))
                && box_region.contains_key(&tag_from_str("wdth"))
            {
                found_both = true;
                assert_eq!(substs.len(), 2);
                assert!(substs.contains_key(&SmolStr::new("dollar")));
                assert!(substs.contains_key(&SmolStr::new("cent")));
            } else if box_region.contains_key(&tag_from_str("wdth"))
                && !box_region.contains_key(&tag_from_str("wght"))
            {
                found_wdth_only = true;
                assert_eq!(substs.len(), 1);
                assert!(substs.contains_key(&SmolStr::new("cent")));
            } else if box_region.contains_key(&tag_from_str("wght"))
                && !box_region.contains_key(&tag_from_str("wdth"))
            {
                found_wght_only = true;
                assert_eq!(substs.len(), 1);
                assert!(substs.contains_key(&SmolStr::new("dollar")));
            }
        }

        assert!(found_both, "Should have found intersection region");
        assert!(found_wdth_only, "Should have found wdth-only region");
        assert!(found_wght_only, "Should have found wght-only region");
    }

    #[test]
    fn test_split_boxes_overlapping_same_subst() {
        // Test with overlapping regions with same substitution (should merge)
        // Two identical rulesets with same substitution
        let region1 = create_region(&[("wght", 0.5, 1.0)]);
        let region1_dup = create_region(&[("wght", 0.5, 1.0)]);

        let rulesets = vec![
            (region1, "dollar".into(), "dollar.rvrn".into()),
            (region1_dup, "dollar".into(), "dollar.rvrn".into()),
        ];

        let result = split_boxes(&rulesets);

        // Should have just one region since both apply to the same space
        assert!(!result.is_empty());
        for (_region, substs) in result.iter() {
            assert_eq!(substs.len(), 1);
            assert!(substs.contains_key(&SmolStr::new("dollar")));
        }
    }

    #[test]
    fn test_split_boxes_complex_overlap() {
        // More complex test with three rulesets and overlaps
        // Similar to the doctest example from featureVars.py but simplified
        let region1 = create_region(&[("wght", 0.5, 1.0)]);
        let region2 = create_region(&[("wght", 0.5, 1.0)]);
        let region3 = create_region(&[("wdth", 0.5, 1.0)]);

        let rulesets = vec![
            (region1, "dollar".into(), "dollar.rvrn".into()),
            (region2, "dollar".into(), "dollar.rvrn".into()),
            (region3, "cent".into(), "cent.rvrn".into()),
        ];

        let result = split_boxes(&rulesets);

        // Should have regions for:
        // 1. wght and wdth overlap (both dollar and cent apply, but dollar from both rulesets)
        // 2. wdth only (cent applies)
        // 3. wght only (dollar applies)
        assert!(!result.is_empty());

        let mut found_overlap = false;
        for (region, substs) in result.iter() {
            let box_region = &region.0[0];
            if box_region.contains_key(&tag_from_str("wght"))
                && box_region.contains_key(&tag_from_str("wdth"))
            {
                found_overlap = true;
                // Should have both dollar and cent
                assert!(substs.contains_key(&SmolStr::new("dollar")));
                assert!(substs.contains_key(&SmolStr::new("cent")));
            }
        }
        assert!(found_overlap, "Should find overlap region");
    }

    #[test]
    fn test_split_boxes_empty() {
        let rulesets: Vec<(DesignspaceRegion, SmolStr, SmolStr)> = vec![];
        let result = split_boxes(&rulesets);
        // Empty input should give empty result
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_split_boxes_single_ruleset() {
        let region = create_region(&[("wght", 0.5, 1.0)]);
        let rulesets = vec![(region, "a".into(), "a.alt".into())];

        let result = split_boxes(&rulesets);

        // Single ruleset should produce one region
        assert_eq!(result.len(), 1);
        for (_region, substs) in result.iter() {
            assert_eq!(substs.len(), 1);
            assert!(substs.contains_key(&SmolStr::new("a")));
        }
    }
}
