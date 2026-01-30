use crate::{
    args::ExistingGlyphHandling,
    error::FontmergeError,
    layout::{closure::LayoutClosureVisitor, visitor::LayoutVisitor},
};
use babelfont::{Font, SmolStr};
use indexmap::{IndexMap, IndexSet};

pub struct GlyphsetFilter {
    pub incoming_glyphset: IndexSet<SmolStr>,
    pub existing_glyphs: IndexSet<SmolStr>,
    mappings_to_delete: IndexMap<SmolStr, Vec<char>>,
    pub existing_glyph_handling: ExistingGlyphHandling,
}

impl GlyphsetFilter {
    /// Create a new GlyphsetFilter
    pub fn new(
        include_glyphs: Vec<SmolStr>,
        exclude_glyphs: Vec<SmolStr>,
        include_codepoints: Vec<char>,
        font1: &mut babelfont::Font,
        font2: &babelfont::Font,
        existing_glyph_handling: ExistingGlyphHandling,
    ) -> Self {
        let mut blacklist: IndexSet<SmolStr> = exclude_glyphs.iter().cloned().collect();
        let mut existing_map = IndexMap::new();
        let existing_glyphs = font1
            .glyphs
            .iter()
            .map(|g| g.name.clone())
            .collect::<IndexSet<SmolStr>>();
        let mut mappings_to_delete: IndexMap<SmolStr, Vec<char>> = IndexMap::new();
        for glyph in font1.glyphs.iter() {
            for cp in glyph.codepoints.iter().flat_map(|cp| char::from_u32(*cp)) {
                existing_map.insert(cp, glyph.name.clone());
            }
        }
        // If there are no include glyphs and no include codepoints, include all glyphs from font2
        let include_glyphs = if include_glyphs.is_empty() && include_codepoints.is_empty() {
            log::info!(
                "No include glyphs or codepoints specified, including all glyphs from font 2"
            );
            font2.glyphs.iter().map(|g| g.name.clone()).collect()
        } else {
            include_glyphs
        };

        let mut incoming_glyphset = IndexSet::new();
        // Add all glyphs selected by include_glyphs
        for glyph_name in include_glyphs.iter() {
            incoming_glyphset.insert(glyph_name.clone());
        }
        // Add all glyphs selected by codepoints
        for glyph in font2.glyphs.iter() {
            if blacklist.contains(&glyph.name) {
                continue;
            }
            for cp in glyph.codepoints.iter().flat_map(|cp| char::from_u32(*cp)) {
                if include_codepoints.contains(&cp) {
                    incoming_glyphset.insert(glyph.name.clone());
                }
                if !incoming_glyphset.contains(&glyph.name) {
                    continue;
                }
                if existing_map.contains_key(&cp) {
                    if existing_glyph_handling == ExistingGlyphHandling::Skip {
                        log::info!(
                            "Skipping glyph '{}' for codepoint U+{:04X} as it exists in font 1",
                            glyph.name,
                            cp as u32
                        );
                        blacklist.insert(glyph.name.clone());
                        continue;
                    } else if existing_glyph_handling == ExistingGlyphHandling::Replace {
                        // Mark Unicode encoding for deletion
                        mappings_to_delete
                            .entry(glyph.name.clone())
                            .or_default()
                            .push(cp);
                    }
                }
            }
        }

        // Remove all blacklisted glyphs from incoming_glyphset
        for glyph_name in blacklist.iter() {
            incoming_glyphset.shift_remove(glyph_name);
        }
        GlyphsetFilter {
            // include_glyphs,
            // exclude_glyphs,
            // include_codepoints,
            // blacklist,
            incoming_glyphset,
            existing_glyphs,
            mappings_to_delete,
            existing_glyph_handling,
        }
    }

    pub(crate) fn de_encode(&self, font_1: &mut Font, font_2: &mut Font) {
        let mut existing_codepoints: IndexSet<u32> = IndexSet::new();
        for glyph in font_1.glyphs.iter_mut() {
            if let Some(codepoints) = self.mappings_to_delete.get(&glyph.name) {
                let codepoints_u32 = codepoints
                    .iter()
                    .map(|c| *c as u32)
                    .collect::<IndexSet<u32>>();
                glyph.codepoints.retain(|cp| !codepoints_u32.contains(cp));
                log::info!(
                    "De-encoded codepoints {:?} from glyph '{}'",
                    codepoints_u32,
                    glyph.name
                );
            }
            existing_codepoints.extend(glyph.codepoints.iter());
        }
        // Any glyphs which have got picked up via e.g. layout closure or component closure
        // should not have codepoints in font2 if they already have codepoints in font1. So
        // check though the incoming glyphset again.
        for glyph in self.incoming_glyphset.iter() {
            if let Some(glyph_in_font2) = font_2.glyphs.get_mut(glyph) {
                glyph_in_font2
                    .codepoints
                    .retain(|cp| !existing_codepoints.contains(cp));
            }
        }
    }

    pub(crate) fn check_for_presence(&mut self, font_2: &Font) {
        let font2_glyphs = font_2
            .glyphs
            .iter()
            .map(|g| g.name.clone())
            .collect::<IndexSet<SmolStr>>();
        let not_there: IndexSet<SmolStr> = self
            .incoming_glyphset
            .difference(&font2_glyphs)
            .cloned()
            .collect();
        if !not_there.is_empty() {
            log::warn!(
                "The following glyphs were selected for inclusion but are not present in font 2: {:?}",
                not_there
            );
            // Remove them from incoming_glyphset
            for glyph_name in not_there {
                self.incoming_glyphset.shift_remove(&glyph_name);
            }
        }
    }

    #[allow(dead_code)] // We'll do this glyph-by-glyph policy one day, but not today
    fn policy(&self, _glyph_name: &str) -> ExistingGlyphHandling {
        self.existing_glyph_handling
    }

    pub(crate) fn close_components(&mut self, font_2: &Font) {
        for glyph in self.incoming_glyphset.clone().iter() {
            self._close_components(glyph, font_2);
        }
    }

    fn _close_components(&mut self, glyph_name: &str, font_2: &Font) {
        let Some(glyph) = font_2.glyphs.get(glyph_name) else {
            return;
        };
        let component_set = glyph
            .layers
            .iter()
            .flat_map(|layer| layer.shapes.iter())
            .filter_map(|shape| match shape {
                babelfont::Shape::Component(comp) => Some(comp.reference.clone()),
                _ => None,
            })
            .collect::<IndexSet<SmolStr>>();
        if component_set.is_empty() {
            return;
        }
        for component_name in component_set.iter() {
            if self.incoming_glyphset.contains(component_name) {
                continue;
            }
            if self.existing_glyphs.contains(component_name) {
                if self.existing_glyph_handling == ExistingGlyphHandling::Replace {
                    log::info!(
                        "Replacing component glyph '{}' used in glyph '{}' already present in font 1",
                        component_name,
                        glyph_name
                    );

                    self.incoming_glyphset.insert(component_name.clone());
                    self._close_components(component_name, font_2);
                } else {
                    log::warn!(
                        "Component glyph '{}' used in glyph '{}' is already present in font 1, not replacing it",
                        component_name,
                        glyph_name
                    );
                }
            } else {
                log::info!(
                    "Adding component glyph '{}' used in glyph '{}' to incoming glyphset",
                    component_name,
                    glyph_name
                );
                self.incoming_glyphset.insert(component_name.clone());
                // Recursively check components of this component
                self._close_components(component_name, font_2);
            }
        }
    }

    /// Sort the incoming glyphset to match the order in font_2
    pub(crate) fn sort_glyphset(&mut self, font_2: &mut Font) {
        let font2_glyphorder = font_2
            .glyphs
            .iter()
            .map(|g| g.name.clone())
            .collect::<Vec<SmolStr>>();
        self.incoming_glyphset.sort_by_key(|g| {
            font2_glyphorder
                .iter()
                .position(|name| name == g)
                .unwrap_or(usize::MAX)
        });
    }

    pub(crate) fn perform_layout_closure(
        &mut self,
        features: &babelfont::Features,
        glyph_names: &[&SmolStr],
        project_root: impl Into<std::path::PathBuf>,
    ) -> Result<(), FontmergeError> {
        let parse_tree =
            crate::layout::get_parse_tree(&features.to_fea(), glyph_names, project_root)?;
        let mut count = self.incoming_glyphset.len();
        let mut rounds = 0;
        loop {
            let mut visitor =
                LayoutClosureVisitor::new(&parse_tree, self.incoming_glyphset.clone());
            visitor.visit();
            self.incoming_glyphset = visitor.glyphset.clone();
            rounds += 1;
            if self.incoming_glyphset.len() == count {
                break;
            }
            if rounds > 10 {
                return Err(FontmergeError::LayoutClosureError);
            }
            count = self.incoming_glyphset.len();
        }
        Ok(())
    }

    pub(crate) fn final_glyphset(&self) -> Vec<SmolStr> {
        self.existing_glyphs
            .union(&self.incoming_glyphset)
            .cloned()
            .collect()
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use crate::args::GlyphSelection;

    use super::*;

    #[test]
    fn test_glyphset() {
        let gs: GlyphSelection = GlyphSelection {
            glyphs: vec![],
            glyphs_file: None,
            codepoints: None,
            codepoints_file: Some("../babelfont/resources/codepoints.txt".to_string()),
            exclude_glyphs: vec![],
            exclude_glyphs_file: None,
        };
        let mut font1 = babelfont::load("../babelfont/resources/NotoSansLimbu.glyphs").unwrap();
        let font2 = babelfont::load("../babelfont/resources/NotoSans.glyphspackage").unwrap();
        let filter = GlyphsetFilter::new(
            gs.get_include_glyphs()
                .expect("Failed to get include glyphs"),
            vec![],
            gs.get_codepoints().expect("Failed to get codepoints"),
            &mut font1,
            &font2,
            ExistingGlyphHandling::Skip,
        );

        let final_glyphs = filter.final_glyphset();
        assert!(final_glyphs.contains(&"a".into()));
        assert!(final_glyphs.contains(&"uni1904".into()));
        assert!(!final_glyphs.contains(&"softsign-cy".into()));
        assert!(!final_glyphs.contains(&"softsign-cy.loclBGR".into()));
    }
}
