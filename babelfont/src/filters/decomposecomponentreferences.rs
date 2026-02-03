use std::{
    collections::{HashMap, HashSet},
    vec,
};

use fontdrasil::{
    coords::{DesignSpace, Location},
    types::{Axes, Tag},
};
use indexmap::IndexSet;
use smol_str::SmolStr;

use crate::{
    filters::FontFilter, interpolate::interpolate_layer, shape, Component, Glyph, Layer, Shape,
};

/// A glyph with pre-computed effective locations for each layer
#[derive(Debug, Clone)]
struct GlyphWithLocations {
    glyph: Glyph,
    layer_locations: Vec<Option<Location<DesignSpace>>>,
}

/// A filter that decomposes component references into geometry.
/// If `components` is `None`, all component references are decomposed. If
/// `Some`, only references to the listed component glyphs are decomposed.
///
/// Note that the arguments to this are *components*, not composite glyphs.
/// We rewrite the composite glyphs by decomposing their component references.
///
// This is *massively* complicated by the need to handle smart components.
pub struct DecomposeComponentReferences(Option<IndexSet<SmolStr>>);

impl DecomposeComponentReferences {
    /// Create a new decomposition filter. If `components` is `None`, all
    /// components will be decomposed; otherwise, only references to the listed
    /// component glyphs are decomposed.
    pub fn new<I, S>(components: Option<I>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<SmolStr>,
    {
        let components = components.map(|names| names.into_iter().map(Into::into).collect());
        DecomposeComponentReferences(components)
    }

    /// Create a filter that decomposes all component references
    pub fn all() -> Self {
        DecomposeComponentReferences(None)
    }
}

struct DecompositionManager<'a> {
    components_filter: Option<IndexSet<SmolStr>>,
    fontdrasil_axes: &'a [fontdrasil::types::Axis],
    // Reusable buffers to avoid repeated allocations
    tasks: Vec<DecomposeTask>,
    glyphs_needed: HashMap<SmolStr, Option<GlyphWithLocations>>,
    visited: HashSet<SmolStr>,
}

impl<'a> DecompositionManager<'a> {
    fn new(
        components_filter: Option<IndexSet<SmolStr>>,
        fontdrasil_axes: &'a [fontdrasil::types::Axis],
    ) -> Self {
        DecompositionManager {
            components_filter,
            fontdrasil_axes,
            tasks: Vec::new(),
            glyphs_needed: HashMap::new(),
            visited: HashSet::new(),
        }
    }

    fn fully_decompose(
        &mut self,
        font: &mut crate::Font,
        glyph_to_decompose: SmolStr,
    ) -> Result<(), crate::BabelfontError> {
        loop {
            // Clear buffers but keep capacity for reuse
            self.tasks.clear();
            self.glyphs_needed.clear();
            self.visited.clear();

            // Collect tasks only for this glyph
            self.collect_tasks_recursive(&glyph_to_decompose, font)?;

            if self.tasks.is_empty() {
                break;
            }

            // Replace glyphs_needed with the CURRENT font versions (not cached)
            // This ensures we use the current state of decomposed glyphs
            for (glyph_name, glyph_ref) in self.glyphs_needed.iter_mut() {
                if let Some(current) = font.glyphs.get(glyph_name) {
                    // Recompute layer locations with the current glyph version
                    let layer_locations: Vec<Option<Location<DesignSpace>>> = current
                        .layers
                        .iter()
                        .map(|layer| layer.effective_location(font))
                        .collect();
                    *glyph_ref = Some(GlyphWithLocations {
                        glyph: current.clone(),
                        layer_locations,
                    });
                }
            }

            // Sort tasks by layer and shape (descending)
            self.tasks.sort_by(|a, b| {
                a.layer_index
                    .cmp(&b.layer_index)
                    .then(b.shape_index.cmp(&a.shape_index)) // descending: high indices first
            });

            // Execute tasks
            for task in self.tasks.iter() {
                task.execute_task(font, &self.glyphs_needed, self.fontdrasil_axes)?;
            }
        }
        Ok(())
    }

    /// If we have a component filter, expand it to include all transitive dependencies
    fn expand_filter_transitively(&mut self, font: &mut crate::Font) {
        self.components_filter = self.components_filter.as_ref().map(|filter| {
            let mut expanded = filter.clone();
            let mut to_visit: Vec<SmolStr> = filter.iter().cloned().collect();
            let mut visited = HashSet::new();

            while let Some(component_name) = to_visit.pop() {
                if visited.contains(&component_name) {
                    continue;
                }
                visited.insert(component_name.clone());

                // Find all components this component references
                if let Some(glyph) = font.glyphs.get(&component_name) {
                    for layer in &glyph.layers {
                        for shape in &layer.shapes {
                            if let Shape::Component(comp) = shape {
                                if !expanded.contains(&comp.reference) {
                                    expanded.insert(comp.reference.clone());
                                    to_visit.push(comp.reference.clone());
                                }
                            }
                        }
                    }
                }
            }
            expanded
        });
    }

    fn should_decompose(&self, component: &Component) -> bool {
        self.components_filter
            .as_ref()
            .map(|set| set.contains(&component.reference))
            .unwrap_or(true)
    }

    /// Recursively collect smart component decomposition tasks in depth-first order
    fn collect_tasks_recursive(
        &mut self,
        glyph_name: &SmolStr,
        font: &crate::Font,
    ) -> Result<(), crate::BabelfontError> {
        // If we've already processed this glyph, skip it to avoid cycles
        if self.visited.contains(glyph_name) {
            return Ok(());
        }
        self.visited.insert(glyph_name.clone());

        let Some(glyph) = font.glyphs.get(glyph_name) else {
            // Referenced glyph doesn't exist, skip it
            return Ok(());
        };

        for (layer_index, layer) in glyph.layers.iter().enumerate() {
            for (shape_index, shape) in layer.shapes.iter().enumerate() {
                if let shape::Shape::Component(component) = shape {
                    // Recursively process the referenced glyph first (depth-first)
                    self.collect_tasks_recursive(&component.reference, font)?;

                    if !self.should_decompose(component) {
                        continue;
                    }

                    // Now add this component to the task list
                    // Pre-compute effective locations for all layers of the referenced glyph
                    if let Some(ref_glyph) = font.glyphs.get(&component.reference) {
                        let layer_locations: Vec<Option<Location<DesignSpace>>> = ref_glyph
                            .layers
                            .iter()
                            .map(|layer| layer.effective_location(font))
                            .collect();
                        self.glyphs_needed.insert(
                            component.reference.clone(),
                            Some(GlyphWithLocations {
                                glyph: ref_glyph.clone(),
                                layer_locations,
                            }),
                        );
                    } else {
                        self.glyphs_needed.insert(component.reference.clone(), None);
                    }

                    self.tasks.push(DecomposeTask {
                        glyph_name: glyph_name.clone(),
                        layer_index,
                        shape_index,
                        referenced_glyph: component.reference.clone(),
                        layer_location: layer.effective_location(font),
                    });
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct DecomposeTask {
    glyph_name: SmolStr,
    layer_index: usize,
    shape_index: usize,
    referenced_glyph: SmolStr,
    layer_location: Option<Location<DesignSpace>>,
}

impl DecomposeTask {
    fn get_layer<'a>(&self, font: &'a crate::Font) -> Result<&'a Layer, crate::BabelfontError> {
        let glyph = font.glyphs.get(&self.glyph_name).ok_or_else(|| {
            crate::BabelfontError::GlyphNotFound {
                glyph: self.glyph_name.to_string(),
            }
        })?;
        let layer = glyph.layers.get(self.layer_index).ok_or_else(|| {
            crate::BabelfontError::FilterError(format!(
                "Layer index {} out of bounds for glyph {}",
                self.layer_index, self.glyph_name
            ))
        })?;
        Ok(layer)
    }
    fn get_layer_mut<'a>(
        &self,
        font: &'a mut crate::Font,
    ) -> Result<&'a mut Layer, crate::BabelfontError> {
        let glyph_mut = font.glyphs.get_mut(&self.glyph_name).ok_or_else(|| {
            crate::BabelfontError::GlyphNotFound {
                glyph: self.glyph_name.to_string(),
            }
        })?;
        let layer_mut = glyph_mut.layers.get_mut(self.layer_index).ok_or_else(|| {
            crate::BabelfontError::FilterError(format!(
                "Layer index {} out of bounds for glyph {}",
                self.layer_index, self.glyph_name
            ))
        })?;
        Ok(layer_mut)
    }
    fn execute_task(
        &self,
        font: &mut crate::Font,
        glyphs_needed: &HashMap<SmolStr, Option<GlyphWithLocations>>,
        fontdrasil_axes: &[fontdrasil::types::Axis],
    ) -> Result<(), crate::BabelfontError> {
        let layer = self.get_layer(font)?;
        let shape = layer.shapes.get(self.shape_index).ok_or_else(|| {
            crate::BabelfontError::FilterError(format!(
                "Shape index {} out of bounds for glyph {} layer {}",
                self.shape_index, self.glyph_name, self.layer_index
            ))
        })?;
        #[allow(clippy::unwrap_used)] // We checked this above
        let glyph_with_locations = glyphs_needed
            .get(&self.referenced_glyph)
            .unwrap()
            .as_ref()
            .ok_or_else(|| crate::BabelfontError::GlyphNotFound {
                glyph: self.referenced_glyph.to_string(),
            })?;
        let decomposed_shapes = decompose_smart_component_with_glyph(
            shape,
            &glyph_with_locations.glyph,
            &glyph_with_locations.layer_locations,
            self.layer_location.as_ref(),
            fontdrasil_axes,
        )?;
        let layer_mut = self.get_layer_mut(font)?;
        layer_mut.shapes.remove(self.shape_index);
        for (i, new_shape) in decomposed_shapes.into_iter().enumerate() {
            layer_mut.shapes.insert(self.shape_index + i, new_shape);
        }
        Ok(())
    }
}

/// Topologically visit glyphs based on component references
fn visit_glyph(
    g: &SmolStr,
    references: &HashMap<SmolStr, HashSet<SmolStr>>,
    all_glyphs: &HashSet<SmolStr>,
    visited: &mut HashSet<SmolStr>,
    result: &mut Vec<SmolStr>,
) {
    if visited.contains(g) {
        return;
    }
    visited.insert(g.clone());

    if let Some(refs) = references.get(g) {
        for referenced in refs {
            if all_glyphs.contains(referenced) {
                visit_glyph(referenced, references, all_glyphs, visited, result);
            }
        }
    }
    result.push(g.clone());
}

impl FontFilter for DecomposeComponentReferences {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        log::info!("Decomposing component references");
        let fontdrasil_axes = font
            .axes
            .iter()
            .map(|ax| ax.clone().try_into())
            .collect::<Result<Vec<_>, _>>()?;

        let mut manager = DecompositionManager::new(self.0.clone(), &fontdrasil_axes);
        manager.expand_filter_transitively(font);

        let mut all_tasks = vec![];

        // Collect all glyphs to scan for component references
        // Build dependency graph for topological sorting
        for glyph in font.glyphs.iter() {
            manager.collect_tasks_recursive(&glyph.name, font)?;
            all_tasks.append(&mut manager.tasks);
            manager.visited.clear();
        }

        // Build dependency graph
        let mut references: HashMap<SmolStr, HashSet<SmolStr>> = HashMap::new();
        for task in &all_tasks {
            references
                .entry(task.glyph_name.clone())
                .or_default()
                .insert(task.referenced_glyph.clone());
        }

        // Topologically sort glyphs to determine processing order
        let all_glyphs_set: HashSet<SmolStr> =
            all_tasks.iter().map(|t| &t.glyph_name).cloned().collect();
        let mut sorted_glyphs = Vec::new();
        let mut visited_sort = HashSet::new();

        for glyph in all_glyphs_set.iter() {
            visit_glyph(
                glyph,
                &references,
                &all_glyphs_set,
                &mut visited_sort,
                &mut sorted_glyphs,
            );
        }
        log::info!("Decomposing {} glyphs", sorted_glyphs.len());

        // Process glyphs in dependency order, fully decomposing each before moving to the next
        for glyph_to_decompose in sorted_glyphs {
            // Fully decompose this glyph
            manager.fully_decompose(font, glyph_to_decompose)?;
        }

        Ok(())
    }

    fn from_str(s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        if s.is_empty() {
            Ok(DecomposeComponentReferences::all())
        } else {
            let glyphs: IndexSet<SmolStr> = s.split(',').map(|g| SmolStr::new(g.trim())).collect();
            Ok(DecomposeComponentReferences::new(Some(glyphs)))
        }
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("decomposecomponents")
            .long("decompose-components")
            .help("Decompose component references (optionally list specific glyphs to decompose)")
            .value_name("GLYPHS")
            .action(clap::ArgAction::Append)
            .default_missing_value("")
            .required(false)
    }
}

fn decompose_smart_component_with_glyph(
    shape: &shape::Shape,
    referenced_glyph: &Glyph,
    referenced_glyph_layer_locations: &[Option<Location<DesignSpace>>],
    layer_location: Option<&Location<DesignSpace>>,
    font_level_axes: &[fontdrasil::types::Axis],
) -> Result<Vec<shape::Shape>, crate::BabelfontError> {
    let Shape::Component(component) = shape else {
        panic!("Can't happen");
    };

    // Fast path: if the referenced glyph has only one layer and no axes,
    // and we're not passing through any external variation, just copy the shapes directly
    if referenced_glyph.layers.len() == 1
        && referenced_glyph.component_axes.is_empty()
        && font_level_axes.is_empty()
    {
        return Ok(referenced_glyph.layers[0]
            .shapes
            .iter()
            .map(|s| s.apply_transform(component.transform))
            .collect());
    }

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
    // At this point we also need to pass in any external axes from the parent glyph's layer
    if let Some(layer_location) = layer_location {
        for (tag, value) in layer_location.iter() {
            target_location.insert(*tag, *value);
        }
    }

    // Make a fake Axes for the designspace. We'll make *our* Axis types, then
    // convert to fontdrasil's.
    let mut axes: Vec<fontdrasil::types::Axis> = referenced_glyph
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
        .collect::<Result<Vec<fontdrasil::types::Axis>, _>>()?;
    // Include font-level axes too
    axes.extend_from_slice(font_level_axes);
    let axes = Axes::new(axes);
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
        .enumerate()
        .filter(|(_, layer)| {
            !layer.smart_component_location.is_empty()
                || layer.location.is_some()
                || matches!(layer.master, crate::LayerType::DefaultForMaster(_))
        })
        .map(|(layer_idx, layer)| {
            let mut loc = Location::new();
            // XXX There's a fit_to_axes method that might be relevant here?
            for axis in axes.iter() {
                loc.insert(axis.tag, axis.default.to_design(&axis.converter));
            }
            for (axis_name, value) in &layer.smart_component_location {
                if let Some(tag) = tags_for_axes.get(axis_name) {
                    loc.insert(*tag, *value);
                }
            }
            // And add the effective location of the layer too
            if let Some(eff_loc) = &referenced_glyph_layer_locations[layer_idx] {
                for (tag, value) in eff_loc.iter() {
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
    let normalized_location = target_location.to_normalized(&axes);
    // Now we can hand the designspace off to our interpolation engine
    let interpolated_layer = interpolate_layer(
        referenced_glyph.name.as_str(),
        &layers_locations,
        &axes,
        &normalized_location?,
    )?;
    // I guess we should also think about the anchors of the component glyphs etc? Not sure.
    Ok(interpolated_layer
        .shapes
        .iter()
        .map(|s| s.apply_transform(component.transform))
        .collect::<Vec<shape::Shape>>())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use crate::filters::RetainGlyphs;

    use super::*;

    #[test]
    fn test_decompose() {
        let mut font = crate::load("noto-cjk-varco/notosanscjksc.rcjk").unwrap();
        let decomposer = DecomposeComponentReferences::all();
        decomposer.apply(&mut font).unwrap();
        font.save("decomposed-rcjk.babelfont").unwrap();
        // There should not be any more smart components
        for glyph in font.glyphs.iter() {
            for layer in glyph.layers.iter() {
                for shape in layer.shapes.iter() {
                    if let shape::Shape::Component(component) = shape {
                        assert!(
                            component.location.is_empty(),
                            "Glyph {} still has smart component",
                            glyph.name
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_subset_is_same_as_whole() {
        log::debug!("Small subset:\n");
        let mut font = crate::load("noto-cjk-varco/notosanscjksc.rcjk").unwrap();
        let master_ids_at_load = font
            .masters
            .iter()
            .map(|m| m.id.clone())
            .collect::<Vec<_>>();
        eprintln!("Master IDs at load: {:?}", master_ids_at_load);
        let mut wanted_glyphs = vec!["uni4E01".into(), "VG_4E85_00".into(), "VG_4E00_00".into()];

        let subset = RetainGlyphs::new(wanted_glyphs.clone());
        subset.apply(&mut font).unwrap();
        eprintln!("Ran subsetter");
        let decomposer = DecomposeComponentReferences::all();
        decomposer.apply(&mut font).unwrap();
        eprintln!("Ran decomposer");
        let master_ids = font
            .masters
            .iter()
            .map(|m| m.id.clone())
            .collect::<Vec<_>>();
        eprintln!("Master IDs after decompose: {:?}", master_ids);
        assert_eq!(
            master_ids_at_load, master_ids,
            "Master order changed after decomposition!"
        );
        assert_eq!(font.glyphs.get("uni4E01").unwrap().layers.len(), 2);
        assert_eq!(
            font.glyphs.get("uni4E01").unwrap().layers[0].master,
            crate::LayerType::DefaultForMaster(master_ids[0].clone())
        );

        assert_eq!(
            font.glyphs.get("uni4E01").unwrap().layers[1].master,
            crate::LayerType::DefaultForMaster(master_ids[1].clone())
        );
        let thin_paths: Vec<crate::Path> = font
            .master_layer_for("uni4E01", &font.masters[0])
            .unwrap()
            .paths()
            .cloned()
            .collect();
        let bold_paths: Vec<crate::Path> = font
            .master_layer_for("uni4E01", &font.masters[1])
            .unwrap()
            .paths()
            .cloned()
            .collect();

        log::debug!("Slightly bigger subset:\n");
        let mut full_font = crate::load("noto-cjk-varco/notosanscjksc.rcjk").unwrap();
        // let's see if we can find a subset of the glyphs that causes the problem
        wanted_glyphs.push("T_2FF0_4E01".into());
        let subset = RetainGlyphs::new(wanted_glyphs.clone());
        subset.apply(&mut full_font).unwrap();

        let decomposer = DecomposeComponentReferences::all();
        decomposer.apply(&mut full_font).unwrap();
        let full_thin_paths: Vec<crate::Path> = full_font
            .master_layer_for("uni4E01", &full_font.masters[0])
            .unwrap()
            .paths()
            .cloned()
            .collect();
        let full_bold_paths: Vec<crate::Path> = full_font
            .master_layer_for("uni4E01", &full_font.masters[1])
            .unwrap()
            .paths()
            .cloned()
            .collect();
        assert_eq!(
            serde_json::to_string(&thin_paths).unwrap(),
            serde_json::to_string(&full_thin_paths).unwrap()
        );
        assert_eq!(
            serde_json::to_string(&bold_paths).unwrap(),
            serde_json::to_string(&full_bold_paths).unwrap()
        );
    }

    #[test]
    fn test_half_decomposition() {
        let mut font = crate::load("resources/decomposition-test.babelfont").unwrap();

        // VG_65E5_00 references VG_53E3_00 and VG_4E00_00, unwrap those first
        let decomposer = DecomposeComponentReferences::new(Some(vec!["VG_53E3_00", "VG_4E00_00"]));
        decomposer.apply(&mut font).unwrap();

        // Then run it on everything else
        let decomposer = DecomposeComponentReferences::all();
        decomposer.apply(&mut font).unwrap();

        font.save("decomposed-half.glyphs").unwrap();

        // Get the paths of T_2099D_2FF0
        let half_paths: Vec<crate::Path> = font
            .master_layer_for("T_2099D_2FF0", &font.masters[0])
            .unwrap()
            .paths()
            .cloned()
            .collect();

        // Now do it all on one step
        let mut font = crate::load("resources/decomposition-test.babelfont").unwrap();
        let decomposer = DecomposeComponentReferences::all();
        decomposer.apply(&mut font).unwrap();

        font.save("decomposed-full.glyphs").unwrap();

        let full_paths: Vec<crate::Path> = font
            .master_layer_for("T_2099D_2FF0", &font.masters[0])
            .unwrap()
            .paths()
            .cloned()
            .collect();

        // The two results should be the same
        assert_eq!(
            serde_json::to_string(&half_paths).unwrap(),
            serde_json::to_string(&full_paths).unwrap()
        );
    }

    #[test]
    fn test_pass_on_external_axes() {
        // We test that when decomposing a smart component which varies across
        // external axes (font-level axes, not component-level axes), those axes are
        // passed through to the interpolation process, and so we get different results
        // decomposing at different external locations.
        let mut font = crate::load("noto-cjk-varco/notosanscjksc.rcjk").unwrap();
        // uni38D5 is a composite which uses the component T_5F73_2FF0 but doesn't
        // specify any component-level axes. When we decompose it, the two layers
        // should have different shapes because the layers are at different external axes
        // locations and these locations should be passed into the interpolation.
        let decomposer = DecomposeComponentReferences::new(Some(vec!["T_5F73_2FF0"]));
        decomposer.apply(&mut font).unwrap();
        let layer0_paths: Vec<crate::Path> = font
            .master_layer_for("uni38D5", &font.masters[0])
            .unwrap()
            .paths()
            .cloned()
            .collect();
        let layer1_paths: Vec<crate::Path> = font
            .master_layer_for("uni38D5", &font.masters[1])
            .unwrap()
            .paths()
            .cloned()
            .collect();
        assert_ne!(
            serde_json::to_string(&layer0_paths).unwrap(),
            serde_json::to_string(&layer1_paths).unwrap()
        );
    }
}
