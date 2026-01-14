use std::{collections::HashMap, vec};

use fontdrasil::{
    coords::{DesignSpace, Location},
    types::{Axes, Tag},
};
use indexmap::IndexSet;
use smol_str::SmolStr;

use crate::{filters::FontFilter, interpolate::interpolate_layer, shape, Glyph, Layer, Shape};

/// A filter that decomposes smart components into their interpolated shapes
pub struct DecomposeSmartComponents(Option<IndexSet<SmolStr>>);

impl DecomposeSmartComponents {
    /// Create a new DecomposeSmartComponents filter for the given axis tag
    pub fn new(glyphset: Option<IndexSet<SmolStr>>) -> Self {
        DecomposeSmartComponents(glyphset)
    }
}

impl FontFilter for DecomposeSmartComponents {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        // The borrow checker's going to hate us if we're not careful. Let's
        // build a to-do
        #[derive(Debug)]
        struct DecomposeTask {
            glyph_name: SmolStr,
            layer_index: usize,
            shape_index: usize,
            referenced_glyph: SmolStr,
        }
        let mut tasks = vec![];
        let mut glyphs_needed = HashMap::new(); // glyph name -> Glyph
        for glyph in font.glyphs.iter() {
            #[allow(clippy::unwrap_used)] // There's an is_some right there
            if self.0.is_some() && !self.0.as_ref().unwrap().contains(&glyph.name) {
                continue;
            }
            for (layer_index, layer) in glyph.layers.iter().enumerate() {
                for (shape_index, shape) in layer.shapes.iter().enumerate() {
                    if let shape::Shape::Component(component) = shape {
                        if !component.location.is_empty() {
                            glyphs_needed.insert(
                                component.reference.clone(),
                                font.glyphs.get(&component.reference).cloned(),
                            );
                            tasks.push(DecomposeTask {
                                glyph_name: glyph.name.clone(),
                                layer_index,
                                shape_index,
                                referenced_glyph: component.reference.clone(),
                            });
                        }
                    }
                }
            }
        }

        // Run the tasks
        for task in tasks {
            let glyph = font.glyphs.get_mut(&task.glyph_name).ok_or_else(|| {
                crate::BabelfontError::GlyphNotFound {
                    glyph: task.glyph_name.to_string(),
                }
            })?;
            let layer = glyph.layers.get(task.layer_index).ok_or_else(|| {
                crate::BabelfontError::FilterError(format!(
                    "Layer index {} out of bounds for glyph {}",
                    task.layer_index, task.glyph_name
                ))
            })?;
            let shape = layer.shapes.get(task.shape_index).ok_or_else(|| {
                crate::BabelfontError::FilterError(format!(
                    "Shape index {} out of bounds for glyph {} layer {}",
                    task.shape_index, task.glyph_name, task.layer_index
                ))
            })?;
            #[allow(clippy::unwrap_used)] // We checked this above
            let referenced_glyph = glyphs_needed
                .get(&task.referenced_glyph)
                .unwrap()
                .as_ref()
                .ok_or_else(|| crate::BabelfontError::GlyphNotFound {
                    glyph: task.referenced_glyph.to_string(),
                })?;
            let decomposed_shapes = decompose_smart_component(shape, referenced_glyph)?;
            let glyph_mut = font.glyphs.get_mut(&task.glyph_name).ok_or_else(|| {
                crate::BabelfontError::GlyphNotFound {
                    glyph: task.glyph_name.to_string(),
                }
            })?;
            let layer_mut = glyph_mut.layers.get_mut(task.layer_index).ok_or_else(|| {
                crate::BabelfontError::FilterError(format!(
                    "Layer index {} out of bounds for glyph {}",
                    task.layer_index, task.glyph_name
                ))
            })?;
            // Replace the shape at shape_index with the decomposed shapes
            layer_mut.shapes.remove(task.shape_index);
            for (i, new_shape) in decomposed_shapes.into_iter().enumerate() {
                layer_mut.shapes.insert(task.shape_index + i, new_shape);
            }
        }

        Ok(())
    }
}

fn decompose_smart_component(
    shape: &shape::Shape,
    referenced_glyph: &Glyph,
) -> Result<Vec<shape::Shape>, crate::BabelfontError> {
    // Dummy implementation for borrow checker testing
    let Shape::Component(component) = shape else {
        // I mean this can't happen.
        return Ok(vec![shape.clone()]);
    };
    // We'd really like to use Location<Designspace> for the smart component location,
    // but we only have axis *names*, not tags. (We call them all VARC as a placeholder)
    // So we're going to make some fake tags.
    let tags_for_axes = referenced_glyph
        .component_axes
        .iter()
        .enumerate()
        .map(|(i, axis)| {
            if i > 999 {
                panic!("Not even David Berlow needs this many axes.");
            }
            #[allow(clippy::unwrap_used)] // We know the format is valid
            let tag: Tag = Tag::new_checked(format!("x{:03}", i).as_bytes()).unwrap();

            (
                axis.name
                    .get_default()
                    .map(|x| x.to_string())
                    .unwrap_or_else(|| "VARC".to_string()),
                tag,
            )
        })
        .collect::<HashMap<String, Tag>>();

    // Now we get the location we want to decompose at, same trick with fake axes
    let mut target_location: Location<DesignSpace> = Location::new();
    for (axis_name, value) in &component.location {
        if let Some(tag) = tags_for_axes.get(axis_name) {
            target_location.insert(*tag, *value);
        }
    }
    // Make a fake Axes for the designspace. We'll make *our* Axis types, then
    // convert to fontdrasil's.
    let axes: Result<Vec<fontdrasil::types::Axis>, _> = referenced_glyph
        .component_axes
        .clone()
        .into_iter()
        .map(|mut axis| {
            #[allow(clippy::unwrap_used)] // We know the tag exists because we made it
            let tag = tags_for_axes
                .get(
                    &axis
                        .name
                        .get_default()
                        .map(|x| x.to_string())
                        .unwrap_or_else(|| "VARC".to_string()),
                )
                .unwrap();
            axis.tag = *tag;
            axis.try_into()
        })
        .collect::<Result<Vec<fontdrasil::types::Axis>, _>>();
    let axes = Axes::new(axes?);
    // Fill in any missing defaults in target_location
    for axis in axes.iter() {
        if !target_location.contains(axis.tag) {
            target_location.insert(axis.tag, axis.default.to_design(&axis.converter));
        }
    }
    // Next we build the designspace model for the layers
    let layers_locations: Vec<(Location<DesignSpace>, &Layer)> = referenced_glyph
        .layers
        .iter()
        .map(|layer| {
            let mut loc = Location::new();
            for axis in axes.iter() {
                loc.insert(axis.tag, axis.default.to_design(&axis.converter));
            }
            for (axis_name, value) in &layer.smart_component_location {
                if let Some(tag) = tags_for_axes.get(axis_name) {
                    loc.insert(*tag, *value);
                }
            }
            (loc, layer)
        })
        .collect();
    log::debug!(
        "Decomposing smart component {} at location {:?}",
        referenced_glyph.name,
        target_location
    );
    log::debug!(
        "Normalized location: {:?}",
        target_location.to_normalized(&axes)
    );
    log::debug!(
        "'Master' locations are: {:?}",
        layers_locations
            .iter()
            .map(|(loc, layer)| format!(
                "{}: {:?}",
                layer.name.as_ref().unwrap_or(layer.id.as_ref().unwrap()),
                loc
            ))
            .collect::<Vec<_>>()
    );
    let normalized_location = target_location.to_normalized(&axes);
    // Now we can hand the designspace off to our interpolation engine
    let interpolated_layer = interpolate_layer(
        referenced_glyph.name.as_str(),
        &layers_locations,
        &axes,
        &normalized_location,
    )?;
    // I guess we should also think about the anchors of the component glyphs etc? Not sure.
    Ok(interpolated_layer
        .shapes
        .iter()
        .map(|s| s.apply_transform(component.transform))
        .collect::<Vec<shape::Shape>>())
}
