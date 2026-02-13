use babelfont::{
    close_layout,
    filters::{DropFeatures, FontFilter as _, ResolveIncludes, RetainGlyphs},
};
use fea_rs_ast::{
    AsFea as _,
    fea_rs::{self, GlyphMap},
};
use indexmap::IndexSet;
use indicatif::ProgressIterator;
mod args;
mod designspace;
mod error;
mod glyphset;
mod kerning;
mod layout;
mod merge;

pub use crate::{
    args::{Args, ExistingGlyphHandling, LayoutHandling},
    glyphset::GlyphsetFilter,
};
use crate::{
    designspace::{add_needed_masters, fontdrasil_axes, map_designspaces, sanity_check},
    kerning::merge_kerning,
    merge::merge_glyph,
};
use babelfont::SmolStr;

use std::path::PathBuf;

pub fn fontmerge(
    mut font1: babelfont::Font,
    font2: babelfont::Font,
    mut glyphset_filter: glyphset::GlyphsetFilter,
    layout_handling: LayoutHandling,
) -> Result<babelfont::Font, error::FontmergeError> {
    glyphset_filter.check_for_presence(&font2);
    let existing_handling = glyphset_filter.existing_glyph_handling;

    let font2_glyphnames = font2
        .glyphs
        .iter()
        .map(|g| &g.name)
        .collect::<Vec<&SmolStr>>();
    let font1_root = font1
        .source
        .as_ref()
        .and_then(|p| p.parent())
        .unwrap_or(std::path::Path::new("."));
    if layout_handling == LayoutHandling::Closure {
        let closed_glyphset = close_layout(&font2, font2_glyphnames.into_iter().cloned().collect())
            .expect("Failed to perform layout closure");
        glyphset_filter.incoming_glyphset = closed_glyphset.iter().cloned().collect();
    }

    if glyphset_filter.incoming_glyphset.is_empty() {
        log::warn!("No glyphs selected for merging from font 2; exiting");
        return Err(error::FontmergeError::NoGlyphsSelected);
    }

    // Babelfont can slim down the font for us
    let mut font2 = font2.clone();
    if layout_handling == LayoutHandling::Ignore {
        // Drop all features first
        DropFeatures::new()
            .apply(&mut font2)
            .expect("Failed to drop features");
    } else {
        // Resolve feature includes
        ResolveIncludes::new(None::<PathBuf>)
            .apply(&mut font2)
            .expect("Failed to resolve includes");
    }

    // This performs the layout subsetting
    RetainGlyphs::new(
        glyphset_filter
            .incoming_glyphset
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>(),
    )
    .apply(&mut font2)
    .expect("Failed to retain selected glyphs in font 2");

    sanity_check_features(&font2);

    let final_glyphset = glyphset_filter.final_glyphset();

    if layout_handling != LayoutHandling::Ignore {
        let font1_features = format!(
            "# Features from {}:\n{}",
            font1
                .source
                .as_ref()
                .and_then(|p| p.as_path().to_str())
                .unwrap_or("font 1"),
            font1.features.to_fea()
        );
        let font2_features = format!(
            "# Features from {}:\n{}",
            font2
                .source
                .as_ref()
                .and_then(|p| p.as_path().to_str())
                .unwrap_or("font 2"),
            font2.features.to_fea()
        );
        let merged_features = font1_features + "\n" + &font2_features;
        // Read into fea-rs-ast
        let glyph_names: Vec<&str> = final_glyphset.iter().map(|g| g.as_str()).collect();
        let merged = fea_rs_ast::FeatureFile::new_from_fea(
            &merged_features,
            Some(&glyph_names),
            Some(font1_root),
        )
        .expect("Failed to parse merged features");
        // Split out any languagesystem statements
        let languagesystems: Vec<(String, String)> = merged
            .statements
            .iter()
            .filter_map(|s| {
                if let fea_rs_ast::ToplevelItem::LanguageSystem(ls) = s {
                    Some((ls.script.clone(), ls.language.clone()))
                } else {
                    None
                }
            })
            .collect();
        let other_statements = merged
            .statements
            .into_iter()
            .filter(|s| !matches!(s, fea_rs_ast::ToplevelItem::LanguageSystem(_)))
            .collect::<Vec<fea_rs_ast::ToplevelItem>>();
        // Sort and uniquify languagesystems, keeping DFLT/dflt at end
        let mut unique_languagesystems = IndexSet::new();
        let mut has_dflt = false;
        for ls in languagesystems {
            if ls.0 == "DFLT" && ls.1 == "dflt" {
                has_dflt = true;
            } else {
                unique_languagesystems.insert(ls);
            }
        }
        if has_dflt {
            unique_languagesystems.insert_before(0, ("DFLT".to_string(), "dflt".to_string()));
        }
        let final_statements = unique_languagesystems
            .into_iter()
            .map(|(script, language)| {
                fea_rs_ast::ToplevelItem::LanguageSystem(fea_rs_ast::LanguageSystemStatement::new(
                    script, language,
                ))
            })
            .chain(other_statements);
        let final_fea = final_statements
            .map(|s| fea_rs_ast::ToplevelItem::as_fea(&s, "") + "\n")
            .collect::<String>();
        font1.features = babelfont::Features::from_fea(&final_fea);
        // merge_features(font1, &subsetted_features);
    }

    if font1.upm != font2.upm {
        log::warn!(
            "Font units per em differ: font1={} font2={}",
            font1.upm,
            font2.upm
        );
        // scale font2 glyphs here
        babelfont::filters::ScaleUpem::new(font1.upm)
            .apply(&mut font2)
            .expect("Failed to scale font2 to match font1 units per em");
    }

    glyphset_filter.close_components(&font2);
    glyphset_filter.sort_glyphset(&mut font2);
    glyphset_filter.de_encode(&mut font1, &mut font2);

    add_needed_masters(&mut font1, &mut font2)
        .expect("Failed to add needed masters from font2 to font1");

    // Compute the list of master IDs here first because checking if a master is sparse or not is
    // expensive, we don't want to do it every time. We use IDs rather than masters because we're
    // going to be borrowing f1 both mutably and immutably below.
    let f1_nonsparse_master_ids: Vec<String> = font1
        .masters
        .iter()
        // .filter(|m| !m.is_sparse(&font1)) // XXX
        .map(|m| m.id.clone())
        .collect();

    let mapping = map_designspaces(&font1, &f1_nonsparse_master_ids, &font2)
        .expect("Could not find a designspace mapping strategy");
    log::info!("Designspace mapping strategies:");
    for (strategy, master) in mapping.iter().zip(font1.masters.iter()) {
        log::info!(
            "  For font1's master '{}', we will {}",
            master.name.get_default().unwrap_or(&master.id),
            strategy
        );
    }

    log::debug!(
        "Final glyphset to include from font 2: {:?}",
        glyphset_filter.incoming_glyphset
    );

    let f2_axes = fontdrasil_axes(&font2.axes).expect("Could not interpret font 2 axes");

    // Merge kerning here
    merge_kerning(&mut font1, &mut font2, &glyphset_filter, &mapping)
        .expect("Failed to merge kerning");

    log::info!("Merging glyphs");
    for glyph in glyphset_filter.incoming_glyphset.iter().progress() {
        if existing_handling == ExistingGlyphHandling::Skip
            && font1.glyphs.iter().any(|g| &g.name == glyph)
        {
            log::info!("Skipping existing glyph '{}'", glyph);
            continue;
        }
        set_layer_locations(glyph, &mut font2);
        if let Some(g) = font2.glyphs.get(glyph) {
            merge_glyph(&mut font1, &f1_nonsparse_master_ids, g, &f2_axes, &mapping)
                .expect("Failed to merge glyph");
        }
    }
    assert!(
        sanity_check(&font1),
        "Font failed sanity check after merging glyphs"
    );

    // Handle dotted circle anchors

    sanity_check_features(&font1);
    Ok(font1)
}

fn set_layer_locations(glyph_name: &SmolStr, font: &mut babelfont::Font) {
    let Some(glyph) = font.glyphs.get_mut(glyph_name) else {
        log::warn!(
            "Glyph '{}' not found in font when setting layer locations",
            glyph_name
        );
        return;
    };
    for layer in glyph.layers.iter_mut() {
        if layer.location.is_none() {
            let id = layer.id.as_ref().or(match &layer.master {
                babelfont::LayerType::DefaultForMaster(m) => Some(m),
                babelfont::LayerType::AssociatedWithMaster(m) => Some(m),
                babelfont::LayerType::FreeFloating => None,
            });
            if let Some(mid) = id {
                if let Some(master) = font.masters.iter().find(|m| &m.id == mid) {
                    layer.location = Some(master.location.clone());
                    log::trace!(
                        "Set layer location for glyph '{}' to {:?}",
                        glyph.name,
                        layer.location
                    );
                } else {
                    log::warn!(
                        "Master ID '{}' for glyph '{}' layer not found in font masters",
                        mid,
                        glyph.name
                    );
                }
            } else {
                log::warn!("Layer for glyph '{}' does not have a master ID", glyph.name);
            }
        } else {
            log::debug!(
                "Layer location for glyph '{}' already set to {:?}",
                glyph.name,
                layer.location
            );
        }
    }
}

fn sanity_check_features(font: &babelfont::Font) {
    let features_text = font.features.to_fea();
    #[allow(clippy::unwrap_used)] // We loaded the font from a file, so it has a source path
    let resolver: Box<dyn fea_rs::parse::SourceResolver> = Box::new(
        fea_rs::parse::FileSystemResolver::new(font.source.clone().unwrap()),
    );
    let glyph_map = GlyphMap::from_iter(font.glyphs.iter().map(|g| g.name.as_str()));
    let (parse_tree, diagnostics) = fea_rs::parse::parse_root(
        "get_parse_tree".into(),
        Some(&glyph_map),
        Box::new(move |s: &std::path::Path| {
            if s == std::path::Path::new("get_parse_tree") {
                Ok(std::sync::Arc::<str>::from(features_text.clone()))
            } else {
                let path = resolver.resolve_raw_path(s.as_ref(), None);
                let canonical = resolver.canonicalize(&path)?;
                resolver.get_contents(&canonical)
            }
        }),
    )
    .expect("Failed to parse features for sanity check");
    if diagnostics.has_errors() {
        log::error!("Errors encountered while parsing feature file for sanity check:");
        log::error!("{}", diagnostics.display());
        return;
    }
    // Validate
    let diagnostics = fea_rs::compile::validate(
        &parse_tree,
        &glyph_map,
        Some(&fea_rs::compile::NopVariationInfo),
    );
    if !diagnostics.is_empty() {
        log::warn!("warns encountered while validating feature file for sanity check:");
        log::warn!("{}", diagnostics.display());
    }
}
