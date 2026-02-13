use crate::layout::renamer::GlyphRenamerVisitor;
use crate::{filters::FontFilter, Features};
use fea_rs_ast::{AsFea, FeatureFile, LayoutVisitor};
use regex::Regex;
use smol_str::SmolStr;
use std::{collections::HashMap, sync::LazyLock};

// Glyph names are [\w_\.]+; we fold in @ to avoid matching classes.
#[allow(clippy::unwrap_used)]
static GLYPH_NAME_MATCHER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([@#][\w_\.]+)\b").unwrap());

/// A filter that renames glyphs in the font
pub struct RenameGlyphs(HashMap<SmolStr, SmolStr>);

impl RenameGlyphs {
    /// Create a new RenameGlyphs filter
    pub fn new(glyph_names: HashMap<String, String>) -> Self {
        RenameGlyphs(
            glyph_names
                .into_iter()
                .map(|(k, v)| (SmolStr::from(k), SmolStr::from(v)))
                .collect(),
        )
    }
}

impl FontFilter for RenameGlyphs {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        log::info!("Renaming glyphs: {:?}", self.0);
        // Update glyph names
        for glyph in font.glyphs.iter_mut() {
            if let Some(new_name) = self.0.get(&glyph.name) {
                log::debug!("Renaming glyph {} to {}", glyph.name, new_name);
                glyph.name = new_name.clone();
            }
        }

        // Update component references
        for glyph in font.glyphs.iter_mut() {
            for layer in &mut glyph.layers {
                if layer.is_background {
                    continue;
                }
                for shape in &mut layer.shapes {
                    if let crate::Shape::Component(comp) = shape {
                        if let Some(new_ref) = self.0.get(&comp.reference) {
                            log::debug!(
                                "Updating component reference {} to {} in glyph {}",
                                comp.reference,
                                new_ref,
                                glyph.name
                            );
                            comp.reference = new_ref.clone();
                        }
                    }
                }
            }
        }
        // Update kerning and kern groups
        for master in font.masters.iter_mut() {
            master.kerning = master
                .kerning
                .iter()
                .map(|((left, right), value)| {
                    let new_left = self.0.get(left).unwrap_or(left);
                    let new_right = self.0.get(right).unwrap_or(right);
                    ((new_left.clone(), new_right.clone()), *value)
                })
                .collect();
        }
        for (_group, members) in font.first_kern_groups.iter_mut() {
            *members = members
                .iter()
                .map(|member| self.0.get(member).unwrap_or(member).clone())
                .collect();
        }
        for (_group, members) in font.second_kern_groups.iter_mut() {
            *members = members
                .iter()
                .map(|member| self.0.get(member).unwrap_or(member).clone())
                .collect();
        }

        // Update features
        let features = font.features.to_fea();
        let glyph_names: Vec<_> = font.glyphs.iter().map(|g| g.name.as_str()).collect();
        let mut feature_file: FeatureFile =
            FeatureFile::new_from_fea(&features, Some(&glyph_names), font.source.clone())
                .map_err(|e| crate::BabelfontError::FilterError(e.to_string()))?;
        let mut visitor = GlyphRenamerVisitor::new(self.0.clone());
        visitor.visit(&mut feature_file).map_err(|e| {
            crate::BabelfontError::FilterError(format!("Error during glyph renaming: {}", e))
        })?;
        font.features = Features::from_fea(&feature_file.as_fea(""));
        // Update classes
        for (_class, definition) in font.features.classes.iter_mut() {
            // be *very* careful here; we need to use a regex to match glyph names, not classes or comments.
            definition.code = GLYPH_NAME_MATCHER
                .replace_all(&definition.code, |caps: &regex::Captures| {
                    let name = &caps[1];
                    if let Some(new_name) = self.0.get(name) {
                        new_name.to_string()
                    } else {
                        name.to_string()
                    }
                })
                .to_string();
        }

        Ok(())
    }

    fn from_str(s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        let glyph_pairs: Vec<String> = s.split(',').map(|g| g.trim().to_string()).collect();
        Ok(RenameGlyphs::new(
            glyph_pairs
                .iter()
                .flat_map(|x| x.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ))
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("renameglyphs")
            .long("rename-glyphs")
            .help("Rename glyphs in the font (comma-separated list of old=new pairs)")
            .value_name("GLYPHS")
            .action(clap::ArgAction::Append)
    }
}
