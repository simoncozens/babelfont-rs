use fontdrasil::{coords::DesignCoord, types::Tag};
use indexmap::IndexMap;

use crate::filters::FontFilter;

#[derive(Hash, PartialEq, Eq)]
struct AxisKey {
    name: String,
    min: Option<u64>,
    default: Option<u64>,
    max: Option<u64>,
}

impl AxisKey {
    fn from_axis(axis: &crate::Axis) -> Self {
        let name = axis.name();
        AxisKey {
            name,
            min: axis.min.as_ref().map(|v| v.to_f64().to_bits()),
            default: axis.default.as_ref().map(|v| v.to_f64().to_bits()),
            max: axis.max.as_ref().map(|v| v.to_f64().to_bits()),
        }
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
        // Gather all unique internal axes, change all the dummy tags to
        // real tags, add to our axes list
        let mut internal_axes = Vec::new();
        let mut names_to_tags: IndexMap<AxisKey, Tag> = IndexMap::new();
        for glyph in font.glyphs.iter_mut() {
            #[allow(clippy::unwrap_used)]
            for component_axis in glyph.component_axes.iter_mut() {
                let axis_key = AxisKey::from_axis(component_axis);
                let counter = internal_axes
                    .iter()
                    .position(|a| a == component_axis)
                    .unwrap_or(internal_axes.len());

                component_axis.tag =
                    Tag::new_checked(format!("V{:0>3}", counter).as_bytes()).unwrap();
                names_to_tags.insert(axis_key, component_axis.tag);
                component_axis.hidden = true;
                if !internal_axes.contains(component_axis) {
                    internal_axes.push(component_axis.clone());
                }
                if !font.axes.contains(component_axis) {
                    font.axes.push(component_axis.clone());
                }
            }
        }
        // Now rewrite all the layers to use the tags
        for glyph in font.glyphs.iter_mut() {
            for layer in glyph.layers.iter_mut() {
                if layer.smart_component_location.is_empty() {
                    continue;
                }
                let mut new_location = IndexMap::new();
                for (axis_name, value) in layer.smart_component_location.iter() {
                    new_location.insert(axis_name.to_string(), *value);
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
                    layer.master = crate::LayerType::FreeFloating;
                    let mut location = layer.location.take().unwrap_or_default();
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
                    // Fill in any missing axes with defaults
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
}
