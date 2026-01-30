use std::collections::HashMap;

use fontdrasil::{coords::DesignCoord, types::Tag};
use indexmap::IndexMap;

use crate::{filters::FontFilter, LayerType};

#[derive(Hash, PartialEq, Eq)]
struct AxisKey {
    name: String,
}

impl AxisKey {
    fn from_axis(axis: &crate::Axis) -> Self {
        let name = axis.name();
        AxisKey { name }
    }
}

/// Normalize a value from an axis's original range to 0-1
fn normalize_axis_value(
    value: f64,
    min: f64,
    default: f64,
    max: f64,
) -> Result<f64, crate::BabelfontError> {
    // Create a temporary axis to use its normalization logic
    let temp_axis = crate::Axis {
        min: Some(fontdrasil::coords::UserCoord::new(min)),
        default: Some(fontdrasil::coords::UserCoord::new(default)),
        max: Some(fontdrasil::coords::UserCoord::new(max)),
        ..Default::default()
    };
    let normalized =
        temp_axis.normalize_userspace_value(fontdrasil::coords::UserCoord::new(value))?;
    Ok(normalized.to_f64())
}

/// Create a normalized version of an axis with range 0-1
fn normalize_axis(axis: &crate::Axis) -> Result<crate::Axis, crate::BabelfontError> {
    let mut normalized = axis.clone();
    normalized.min = Some(fontdrasil::coords::UserCoord::new(0.0));
    normalized.max = Some(fontdrasil::coords::UserCoord::new(1.0));

    // Normalize the default value to the 0-1 range
    if let (Some(orig_min), Some(orig_default), Some(orig_max)) = (axis.min, axis.default, axis.max)
    {
        let normalized_default = normalize_axis_value(
            orig_default.to_f64(),
            orig_min.to_f64(),
            orig_default.to_f64(),
            orig_max.to_f64(),
        )?;
        normalized.default = Some(fontdrasil::coords::UserCoord::new(normalized_default));
    } else {
        // If bounds are not specified, use 0.5 as default
        normalized.default = Some(fontdrasil::coords::UserCoord::new(0.5));
    }

    // Clear the map since we're changing the coordinate space
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
        let mut names_to_tags: IndexMap<AxisKey, Tag> = IndexMap::new();
        for glyph in font.glyphs.iter_mut() {
            #[allow(clippy::unwrap_used)]
            for component_axis in glyph.component_axes.iter_mut() {
                // Normalize the axis to 0-1 range
                let normalized_axis = normalize_axis(component_axis)?;
                *component_axis = normalized_axis;

                let axis_key = AxisKey::from_axis(component_axis);
                let counter = internal_axes
                    .iter()
                    .position(|a: &crate::Axis| AxisKey::from_axis(a) == axis_key)
                    .unwrap_or(internal_axes.len());

                component_axis.tag =
                    Tag::new_checked(format!("V{:0>3}", counter).as_bytes()).unwrap();
                names_to_tags.insert(axis_key, component_axis.tag);
                component_axis.hidden = true;
                if !internal_axes.iter().any(|a: &crate::Axis| {
                    AxisKey::from_axis(a) == AxisKey::from_axis(component_axis)
                }) {
                    internal_axes.push(component_axis.clone());
                }
                if !font
                    .axes
                    .iter()
                    .any(|a| AxisKey::from_axis(a) == AxisKey::from_axis(component_axis))
                {
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
                            if let Some((min, default, max)) =
                                axis_bounds.get(&(ref_glyph_name.clone(), axis_name.clone()))
                            {
                                let normalized_value =
                                    normalize_axis_value(value.to_f64(), *min, *default, *max)?;
                                new_location.insert(
                                    axis_name.clone(),
                                    fontdrasil::coords::DesignCoord::new(normalized_value),
                                );
                            } else {
                                // No bounds found, keep original value
                                new_location.insert(axis_name.clone(), *value);
                            }
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
                    // Normalize the value based on original axis bounds
                    if let Some((min, default, max)) =
                        axis_bounds.get(&(glyph.name.to_string(), axis_name.clone()))
                    {
                        let normalized_value =
                            normalize_axis_value(value.to_f64(), *min, *default, *max)?;
                        new_location.insert(
                            axis_name.clone(),
                            fontdrasil::coords::DesignCoord::new(normalized_value),
                        );
                    } else {
                        // No bounds found, keep original value
                        new_location.insert(axis_name.to_string(), *value);
                    }
                }
                layer.smart_component_location = new_location;
                log::trace!(
                    "New smart_component_location for {}, {}: {:?}",
                    glyph.name,
                    layer.debug_name(),
                    layer.smart_component_location
                );
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
                            let axis_key = AxisKey::from_axis(axis);
                            if let Some(tag) = names_to_tags.get(&axis_key) {
                                location.insert(*tag, DesignCoord::new(value.to_f64()));
                            }
                        }
                    }
                    // Fill in any missing (non-font-level) axes with defaults
                    for axis in font.axes.iter() {
                        if !location.contains(axis.tag) {
                            let default_userspace = axis.default.unwrap_or_default();
                            let default_designspace =
                                axis.userspace_to_designspace(default_userspace)?;
                            location.insert(axis.tag, default_designspace);
                        }
                    }
                    layer.location = Some(location);
                    layer.smart_component_location.clear();
                    // println!("New location: {:?}", layer.location);
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
