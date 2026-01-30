use std::collections::{HashMap, HashSet};

use fontdrasil::{coords::DesignCoord, types::Tag};
use indexmap::IndexMap;

use crate::{filters::FontFilter, LayerType};

/// Normalize a value from an axis's original range to 0-1
fn normalize_axis_value(value: f64, min: f64, default: f64, max: f64) -> f64 {
    // It turns out RoboCJK smart axes sometimes have defaults not at endpoints!
    // So we have to do a piecewise normalization, mapping [min, default] to [-1, 0]
    // and [default, max] to [0, 1.0]
    let normalized = if (value - default).abs() < f64::EPSILON {
        0.0
    } else if value <= default {
        (value - default) / (default - min)
    } else {
        (value - default) / (max - default)
    };
    normalized
}

/// Create a normalized version of an axis with range -1 to 1
fn normalize_axis(axis: &crate::Axis) -> Result<crate::Axis, crate::BabelfontError> {
    let mut normalized = axis.clone();
    normalized.min = Some(fontdrasil::coords::UserCoord::new(-1.0));
    normalized.max = Some(fontdrasil::coords::UserCoord::new(1.0));
    normalized.default = Some(fontdrasil::coords::UserCoord::new(0.0));
    normalized.map = None;
    Ok(normalized)
}

#[derive(Default)]
/// A filter that converts internal axes to real OpenType axes for VARC table generation
pub struct RewriteSmartAxes;

impl RewriteSmartAxes {
    /// Create a new RewriteSmartAxes filter
    pub fn new() -> Self {
        RewriteSmartAxes
    }
}

impl FontFilter for RewriteSmartAxes {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        // We are preparing to export smart components as VARC. Let's ensure that
        // all transitively used components of exported glyphs are themselves set to export.
        let mut to_check = font
            .glyphs
            .iter()
            .filter(|g| g.exported)
            .map(|g| g.name.clone())
            .collect::<Vec<_>>();
        let mut checked = IndexMap::new();
        while let Some(glyph_name) = to_check.pop() {
            if checked.contains_key(&glyph_name) {
                continue;
            }
            checked.insert(glyph_name.clone(), ());
            if let Some(glyph) = font.glyphs.get(&glyph_name) {
                for layer in glyph.layers.iter() {
                    for shape in layer.shapes.iter() {
                        if let crate::Shape::Component(component) = shape {
                            if let Some(ref_glyph) = font.glyphs.get(&component.reference) {
                                if !checked.contains_key(&ref_glyph.name) {
                                    to_check.push(ref_glyph.name.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
        // Now mark all checked glyphs as exported
        for glyphname in checked.keys() {
            if let Some(glyph) = font.glyphs.get_mut(glyphname) {
                glyph.exported = true;
            }
        }

        // First pass: collect original axis bounds for normalization
        // Map from (glyph_name, axis_name) -> (min, default, max)
        let mut axis_bounds: HashMap<(String, String), (f64, f64, f64)> = HashMap::new();
        for glyph in font.glyphs.iter() {
            for component_axis in glyph.component_axes.iter() {
                if let (Some(min), Some(default), Some(max)) = (
                    component_axis.min,
                    component_axis.default,
                    component_axis.max,
                ) {
                    axis_bounds.insert(
                        (glyph.name.to_string(), component_axis.name()),
                        (min.to_f64(), default.to_f64(), max.to_f64()),
                    );
                }
            }
        }

        // Gather all unique internal axes, change all the dummy tags to
        // real tags, add to our axes list
        let mut internal_axes = Vec::new();
        let mut names_to_tags: IndexMap<String, Tag> = IndexMap::new();
        for glyph in font.glyphs.iter_mut() {
            #[allow(clippy::unwrap_used)]
            for component_axis in glyph.component_axes.iter_mut() {
                // Normalize the axis to 0-1 range
                let normalized_axis = normalize_axis(component_axis)?;
                *component_axis = normalized_axis;

                let axis_key = component_axis.name();
                let counter = internal_axes
                    .iter()
                    .position(|a: &crate::Axis| a.name() == axis_key)
                    .unwrap_or(internal_axes.len());

                component_axis.tag =
                    Tag::new_checked(format!("V{:0>3}", counter).as_bytes()).unwrap();
                names_to_tags.insert(axis_key, component_axis.tag);
                component_axis.hidden = true;
                if !internal_axes
                    .iter()
                    .any(|a: &crate::Axis| a.name() == component_axis.name())
                {
                    internal_axes.push(component_axis.clone());
                }
                if !font.axes.iter().any(|a| a.name() == component_axis.name()) {
                    font.axes.push(component_axis.clone());
                }
            }
        }

        // Normalize all component.location values in layers
        for glyph in font.glyphs.iter_mut() {
            for layer in glyph.layers.iter_mut() {
                for shape in layer.shapes.iter_mut() {
                    if let crate::Shape::Component(component) = shape {
                        let ref_glyph_name = component.reference.to_string();
                        let mut new_location = IndexMap::new();
                        for (axis_name, value) in component.location.iter() {
                            convert_axis(
                                &axis_bounds,
                                &ref_glyph_name,
                                &mut new_location,
                                axis_name,
                                value,
                            );
                        }
                        component.location = new_location;
                    }
                }
            }
        }

        // Now rewrite all the layers to use the tags and normalize smart_component_location
        for glyph in font.glyphs.iter_mut() {
            for layer in glyph.layers.iter_mut() {
                if layer.smart_component_location.is_empty() {
                    continue;
                }
                let mut new_location = IndexMap::new();
                for (axis_name, value) in layer.smart_component_location.iter() {
                    convert_axis(
                        &axis_bounds,
                        &glyph.name.to_string(),
                        &mut new_location,
                        axis_name,
                        value,
                    );
                }
                layer.smart_component_location = new_location;
            }
        }
        // Masters need to have default values for the new locations
        for master in font.masters.iter_mut() {
            for internal_axis in internal_axes.iter() {
                if !master.location.contains(internal_axis.tag) {
                    let default_userspace = internal_axis.default.unwrap_or_default();
                    // There is no mapping
                    let default_designspace = DesignCoord::new(default_userspace.to_f64());
                    master
                        .location
                        .insert(internal_axis.tag, default_designspace);
                }
            }
        }
        // As do instances
        for instance in font.instances.iter_mut() {
            for internal_axis in internal_axes.iter() {
                if !instance.location.contains(internal_axis.tag) {
                    let default_userspace = internal_axis.default.unwrap_or_default();
                    // There is no mapping
                    let default_designspace = DesignCoord::new(default_userspace.to_f64());
                    instance
                        .location
                        .insert(internal_axis.tag, default_designspace);
                }
            }
        }
        // For each glyph, track which V axes it actually uses
        let mut glyph_axis_tags: HashMap<String, HashSet<Tag>> = HashMap::new();
        for glyph in font.glyphs.iter() {
            let mut used_tags = HashSet::new();
            for axis in glyph.component_axes.iter() {
                used_tags.insert(axis.tag);
            }
            if !used_tags.is_empty() {
                glyph_axis_tags.insert(glyph.name.to_string(), used_tags);
            }
        }

        // And so layers with their own locations
        for glyph in font.glyphs.iter_mut() {
            for layer in glyph.layers.iter_mut() {
                // Ensure all internal axes have a value
                for internal_axis in internal_axes.iter() {
                    if let Some(location) = &mut layer.location {
                        if !location.contains(internal_axis.tag) {
                            let default_userspace = internal_axis.default.unwrap_or_default();
                            // There is no mapping
                            let default_designspace = DesignCoord::new(default_userspace.to_f64());
                            location.insert(internal_axis.tag, default_designspace);
                        }
                    }
                }
                // Also, the "smart component location" is just the location now
                if !layer.smart_component_location.is_empty() {
                    // inline "effective_location" implementation here because we have a &mut Font, sigh
                    let master_ids = font
                        .masters
                        .iter()
                        .map(|m| (m.id.clone(), m))
                        .collect::<HashMap<_, _>>();
                    let mut location = if let LayerType::DefaultForMaster(mid) = &layer.master {
                        let master = master_ids.get(mid);
                        master.map(|m| m.location.clone())
                    } else {
                        layer.location.take()
                    }
                    .unwrap_or_default();
                    for (axis_name, value) in layer.smart_component_location.iter() {
                        if let Some(axis) =
                            glyph.component_axes.iter().find(|a| a.name() == *axis_name)
                        {
                            let axis_key = axis.name();
                            if let Some(tag) = names_to_tags.get(&axis_key) {
                                location.insert(*tag, DesignCoord::new(value.to_f64()));
                            }
                        } else {
                            log::warn!(
                                "Component axis {} not found in glyph {}!",
                                axis_name,
                                glyph.name
                            );
                        }
                    }
                    log::debug!(
                        "RewriteSmartAxes: After adding smart component values for {}: {:?}",
                        layer.debug_name(),
                        location
                    );
                    // Fill in any missing axes with defaults
                    for axis in font.axes.iter() {
                        if !location.contains(axis.tag) {
                            let default_designspace =
                                axis.userspace_to_designspace(axis.default.unwrap_or_default())?;
                            location.insert(axis.tag, default_designspace);
                        }
                    }
                    layer.location = Some(location);
                    layer.smart_component_location.clear();
                    log::debug!(
                        "New location for glyph {} layer {:?}: {:?}",
                        glyph.name,
                        layer.id,
                        layer.location
                    );
                }
            }
        }

        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(RewriteSmartAxes::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("rewritesmartaxes")
            .long("rewrite-smart-axes")
            .help("Convert internal smart component axes to OpenType variation axes")
            .action(clap::ArgAction::SetTrue)
    }
}

fn convert_axis(
    axis_bounds: &HashMap<(String, String), (f64, f64, f64)>,
    ref_glyph_name: &String,
    new_location: &mut IndexMap<String, fontdrasil::coords::Coord<fontdrasil::coords::DesignSpace>>,
    axis_name: &String,
    value: &fontdrasil::coords::Coord<fontdrasil::coords::DesignSpace>,
) {
    if let Some((min, default, max)) = axis_bounds.get(&(ref_glyph_name.clone(), axis_name.clone()))
    {
        let normalized_value = normalize_axis_value(value.to_f64(), *min, *default, *max);
        new_location.insert(
            axis_name.clone(),
            fontdrasil::coords::DesignCoord::new(normalized_value),
        );
    } else {
        // No bounds found, keep original value
        new_location.insert(axis_name.clone(), *value);
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grantha_unique_locations() {
        let path = "resources/NotoSansGrantha-SmartComponent.glyphs";
        let mut font = crate::load(path).expect("Failed to load babelfont");

        println!("\nBefore RewriteSmartAxes:");
        if let Some(glyph) = font.glyphs.get("_part.iMatra") {
            for (i, layer) in glyph.layers.iter().enumerate() {
                println!(
                    "  Layer {} ({}): location={:?}, smart_component_location={:?}",
                    i,
                    layer.id.as_deref().unwrap_or("?"),
                    layer.location,
                    layer.smart_component_location
                );
            }
        }

        RewriteSmartAxes
            .apply(&mut font)
            .expect("Failed to apply filter");

        println!("\nAfter RewriteSmartAxes:");
        if let Some(glyph) = font.glyphs.get("_part.iMatra") {
            for (i, layer) in glyph.layers.iter().enumerate() {
                println!(
                    "  Layer {} ({}): location={:?}, smart_component_location={:?}",
                    i,
                    layer.id.as_deref().unwrap_or("?"),
                    layer.location,
                    layer.smart_component_location
                );
            }
        }

        println!("\nFont axes:");
        for axis in &font.axes {
            println!(
                "  {} ({}): {:?} <- {:?} -> {:?}",
                axis.name(),
                axis.tag,
                axis.min,
                axis.default,
                axis.max
            );
        }

        let glyph = font.glyphs.get("_part.iMatra").expect("Missing glyph");
        assert_no_duplicate_locations(glyph);
        // This will fail until we write fixup_axis_definitions
        // assert_has_one_default_location(
        //     glyph,
        //     &font.default_location().expect("No default location"),
        // );
    }

    fn assert_no_duplicate_locations(glyph: &crate::Glyph) {
        let mut locations = HashMap::new();
        for layer in glyph.layers.iter() {
            if let Some(loc) = &layer.location {
                if let Some(layerid) = locations.get(loc) {
                    panic!(
                        "Layer {:?} contains duplicate location {:?}, also provided by {:?}",
                        layer.id, loc, layerid
                    );
                }
                locations.insert(loc.clone(), layer.id.clone());
            }
        }
    }

    fn assert_has_one_default_location(
        glyph: &crate::Glyph,
        default_location: &fontdrasil::coords::Location<fontdrasil::coords::DesignSpace>,
    ) {
        let mut found = false;
        for layer in glyph.layers.iter() {
            if let Some(loc) = &layer.location {
                if loc == default_location {
                    if found {
                        panic!(
                            "Glyph {} has multiple layers with default location {:?}",
                            glyph.name, default_location
                        );
                    }
                    found = true;
                }
            }
        }
        if !found {
            panic!(
                "Glyph {} has no layer with default location {:?}; instead, it has layers at: {:?}",
                glyph.name,
                default_location,
                glyph
                    .layers
                    .iter()
                    .filter_map(|layer| layer.location.as_ref())
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn test_robocjk_unique_locations() {
        let path = "resources/notosanscjk.babelfont";
        let mut font = crate::load(path).expect("Failed to load babelfont");
        RewriteSmartAxes
            .apply(&mut font)
            .expect("Failed to apply filter");
        let default_location = font
            .default_location()
            .expect("There was no default location");
        // T_2FF0_4E14 has a width axis (which becomes V000), min/default = 539, max=675
        // and a B_H_left_length axis (which becomes V003), range -100/0/+100
        // The first two layers have smart_component_location "width": 539.0, "B_H_left_length": 0.0
        // and are default for the two masters, layer 1 is the default location for the font.
        let glyph = font.glyphs.get("T_2FF0_4E14").expect("Missing glyph");
        let default_loc = glyph.layers[1].location.as_ref().unwrap();
        // We want to ensure the width location is *default* (0.5), not min (0.0)
        assert_eq!(
            default_loc
                // Of course this test will fail if the axes get renamed...
                .get(Tag::new_checked(b"V000").unwrap())
                .unwrap()
                .to_f64(),
            0.0
        );

        for glyph in font.glyphs.iter() {
            if !glyph.is_smart_component() {
                continue;
            }
            assert_no_duplicate_locations(glyph);
            assert_has_one_default_location(glyph, &default_location);
        }
        // Let's check the new axes make sense
        for axis in &font.axes {
            assert!(axis.tag != Tag::new(b"VARC"));
            if axis.tag.to_string().starts_with("V") {
                assert_eq!(axis.min.unwrap().to_f64(), -1.0);
                assert_eq!(axis.max.unwrap().to_f64(), 1.0);
            }
        }
    }
}
