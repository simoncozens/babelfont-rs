use std::collections::HashMap;

use fontdrasil::{coords::DesignCoord, types::Tag};
use indexmap::IndexMap;

use crate::{filters::FontFilter, LayerType};

/// Normalize a smart component axis value from its original range to 0-1.
fn normalize_axis_value(value: f64, min: f64, max: f64) -> f64 {
    if max == min {
        0.0 // Avoid division by zero
    } else {
        (value - min) / (max - min)
    }
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
        // First pass: collect original axis bounds for normalization
        // Map from (glyph_name, axis_name) -> (min, max)
        let mut axis_bounds: HashMap<(String, String), (f64, f64)> = HashMap::new();
        for glyph in font.glyphs.iter() {
            for component_axis in glyph.component_axes.iter() {
                if let (Some(min), Some(max)) = (component_axis.min, component_axis.max) {
                    axis_bounds.insert(
                        (glyph.name.to_string(), component_axis.name()),
                        (min.to_f64(), max.to_f64()),
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
                // Normalize the axis range to 0-1
                if let (Some(min), Some(max)) = (component_axis.min, component_axis.max) {
                    let min_f64 = min.to_f64();
                    let max_f64 = max.to_f64();
                    component_axis.min = Some(fontdrasil::coords::UserCoord::new(0.0));
                    component_axis.max = Some(fontdrasil::coords::UserCoord::new(1.0));
                    // Normalize the default to the 0-1 range
                    if let Some(default) = component_axis.default {
                        let normalized_default =
                            normalize_axis_value(default.to_f64(), min_f64, max_f64);
                        component_axis.default =
                            Some(fontdrasil::coords::UserCoord::new(normalized_default));
                    }
                }
                component_axis.map = None; // Clear any mapping since we're changing coordinate space

                let axis_name = component_axis.name();
                let counter = internal_axes
                    .iter()
                    .position(|a: &crate::Axis| a.name() == axis_name)
                    .unwrap_or(internal_axes.len());

                component_axis.tag =
                    Tag::new_checked(format!("V{:0>3}", counter).as_bytes()).unwrap();
                names_to_tags.insert(axis_name.clone(), component_axis.tag);
                component_axis.hidden = true;
                if !internal_axes
                    .iter()
                    .any(|a: &crate::Axis| a.name() == axis_name)
                {
                    internal_axes.push(component_axis.clone());
                }
                if !font.axes.iter().any(|a| a.name() == axis_name) {
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
                            if let Some((min, max)) =
                                axis_bounds.get(&(ref_glyph_name.clone(), axis_name.clone()))
                            {
                                let normalized_value =
                                    normalize_axis_value(value.to_f64(), *min, *max);
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
                    if let Some((min, max)) =
                        axis_bounds.get(&(glyph.name.to_string(), axis_name.clone()))
                    {
                        let normalized_value = normalize_axis_value(value.to_f64(), *min, *max);
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
                        if let Some(tag) = names_to_tags.get(axis_name) {
                            location.insert(*tag, DesignCoord::new(value.to_f64()));
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
