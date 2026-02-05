use crate::filters::{DecomposeComponentReferences, FontFilter, SubsetLayout};
use smol_str::SmolStr;
use std::collections::HashSet;

/// A filter that retains only the specified glyphs in a font
///
/// When a glyph is retained, any components it references that are not in the retain list
/// are decomposed. Masters that become sparse as a result are removed, and their associated layers
/// are converted to associated layers of a non-sparse master. Features are also subsetted
/// to only reference the retained glyphs.
pub struct RetainGlyphs(Vec<SmolStr>);

impl RetainGlyphs {
    /// Create a new RetainGlyphs filter
    pub fn new(glyph_names: Vec<String>) -> Self {
        RetainGlyphs(glyph_names.into_iter().map(SmolStr::from).collect())
    }
}

impl FontFilter for RetainGlyphs {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        log::info!("Retaining glyphs: {:?}", self.0);
        // Filter features
        SubsetLayout::new(self.0.clone()).apply(font)?;

        // Find components referenced by retained glyphs that will be dropped
        let mut components_to_decompose = HashSet::new();
        for glyph in font.glyphs.iter() {
            if !self.0.contains(&glyph.name) {
                continue; // Only look at retained glyphs
            }
            for layer in &glyph.layers {
                if layer.is_background {
                    continue;
                }
                for shape in &layer.shapes {
                    if let crate::Shape::Component(comp) = shape {
                        // If this component references a glyph being dropped, mark it for decomposition
                        let reference = comp.reference.as_str();
                        if !self.0.contains(&comp.reference)
                            && !components_to_decompose.contains(reference)
                        {
                            components_to_decompose.insert(reference);
                            log::debug!(
                                "Decomposing component {} used by glyph {}",
                                comp.reference,
                                glyph.name
                            );
                        }
                    }
                }
            }
        }

        // Only decompose if there are components to decompose
        if !components_to_decompose.is_empty() {
            log::info!(
                "Decomposing {} component references",
                components_to_decompose.len()
            );
            log::debug!("Components to decompose: {:?}", components_to_decompose);
            let decomposer = DecomposeComponentReferences::new(Some(
                components_to_decompose.into_iter().collect::<Vec<_>>(),
            ));
            decomposer.apply(font)?;
        }

        // Retain only the specified glyphs
        font.glyphs.retain(|g| self.0.contains(&g.name));
        for (_group, members) in font.first_kern_groups.iter_mut() {
            members.retain(|g| self.0.contains(g));
        }
        for (_group, members) in font.second_kern_groups.iter_mut() {
            members.retain(|g| self.0.contains(g));
        }
        // Drop dead groups
        font.first_kern_groups
            .retain(|_group, members| !members.is_empty());
        font.second_kern_groups
            .retain(|_group, members| !members.is_empty());
        // Filter kerning
        for master in font.masters.iter_mut() {
            master.kerning.retain(|(left, right), _| {
                // Because we removed all the dead groups, any groups still refer to things we care about
                (self.0.contains(left)
                    || (left.starts_with('@') && font.first_kern_groups.contains_key(&left[1..])))
                    && (self.0.contains(right)
                        || (right.starts_with('@')
                            && font.second_kern_groups.contains_key(&right[1..])))
            });
        }
        // Filter masters - remove any masters which were just sparse
        font.masters.retain(|master| {
            font.glyphs.iter().any(|glyph| {
                glyph.layers.iter().any(|layer| {
                    layer.master == crate::LayerType::DefaultForMaster(master.id.clone())
                })
            })
        });

        Ok(())
    }

    fn from_str(s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        let glyph_names: Vec<String> = s.split(',').map(|g| g.trim().to_string()).collect();
        Ok(RetainGlyphs::new(glyph_names))
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("retainglyphs")
            .long("retain-glyphs")
            .help("Retain only the specified glyphs (comma-separated list)")
            .value_name("GLYPHS")
            .action(clap::ArgAction::Append)
    }
}
