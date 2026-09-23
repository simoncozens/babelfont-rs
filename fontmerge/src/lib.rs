//! # fontmerge
//!
//! Merges two font source files together, copying selected glyphs from a *donor*
//! font into a *host* font, along with any necessary OpenType layout features.
//!
//! This is a mixed binary/library crate: the `fontmerge` binary provides a
//! command-line tool, and the library exposes the same merging routine for
//! programmatic use. See the [`fontmerge`](crate::fontmerge) function and the
//! [`Args`](crate::Args) / [`GlyphsetFilter`](crate::GlyphsetFilter) types for
//! the entry points.
use babelfont::{
    Features, GlyphList, close_layout,
    filters::{DropFeatures, FontFilter as _, ResolveIncludes, RetainGlyphs, SubsetVisitor},
};
use fea_rs_ast::{
    AsFea as _, LayoutVisitor as _,
    fea_rs::{self, GlyphMap},
};
use indexmap::IndexSet;
use indicatif::ProgressIterator;
pub mod args;
mod designspace;
mod error;
mod glyphset;
mod kerning;
mod layout;
mod merge;

pub use crate::{
    args::{Args, DuplicateLookupHandling, ExistingGlyphHandling, LayoutHandling},
    glyphset::GlyphsetFilter,
};
use crate::{
    designspace::{add_needed_masters, fontdrasil_axes, map_designspaces, sanity_check},
    kerning::merge_kerning,
    layout::gatherer::NameGathererVisitor,
    merge::merge_glyph,
};
use babelfont::SmolStr;

use std::{collections::HashSet, path::PathBuf};

pub fn fontsubset(
    font1: babelfont::Font,
    glyphset_filter: glyphset::GlyphsetFilter,
    layout_handling: LayoutHandling,
    process_avar_mapping: bool,
    merge_dotted_circle_anchors: bool,
) -> Result<babelfont::Font, error::FontmergeError> {
    let mut target = font1.clone();
    target.features = Features::default();
    target.glyphs = GlyphList::default();
    fontmerge(
        target,
        font1,
        glyphset_filter,
        layout_handling,
        // Subsetting a single font cannot produce duplicate lookups.
        DuplicateLookupHandling::First,
        process_avar_mapping,
        merge_dotted_circle_anchors,
    )
}

pub fn fontmerge(
    mut font1: babelfont::Font,
    font2: babelfont::Font,
    mut glyphset_filter: glyphset::GlyphsetFilter,
    layout_handling: LayoutHandling,
    duplicate_lookups: DuplicateLookupHandling,
    process_avar_mapping: bool,
    merge_dotted_circle_anchors: bool,
) -> Result<babelfont::Font, error::FontmergeError> {
    glyphset_filter.check_for_presence(&font2);
    let existing_handling = glyphset_filter.existing_glyph_handling;

    // Capture the pre-merge dotted circle glyphs from both fonts (if requested) so that we
    // can merge their anchors after the glyph merge has taken place, rather than one glyph's
    // anchors simply clobbering the other's.
    let dotted_circle_pair = merge_dotted_circle_anchors
        .then(|| find_dotted_circle(&font1).zip(find_dotted_circle(&font2)))
        .flatten();

    let font1_root = font1
        .source
        .as_ref()
        .and_then(|p| p.parent())
        .unwrap_or(std::path::Path::new("."));
    if layout_handling == LayoutHandling::Closure {
        let closed_glyphset = close_layout(
            &font2,
            glyphset_filter.incoming_glyphset.into_iter().collect(),
        )
        .expect("Failed to perform layout closure");
        glyphset_filter.incoming_glyphset = closed_glyphset.iter().cloned().collect();
    }

    if glyphset_filter.incoming_glyphset.is_empty() {
        log::warn!("No glyphs selected for merging from font 2; exiting");
        return Err(error::FontmergeError::NoGlyphsSelected);
    }

    // Glyphs referenced as components of selected glyphs need to come along for the ride too.
    // This must happen *before* we ask babelfont to subset font2 down to the selected glyphset,
    // otherwise the component references (and the component glyphs themselves) will already
    // have been decomposed/dropped by the time we get around to looking for them.
    glyphset_filter.close_components(&font2);

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

    let final_glyphset = glyphset_filter.final_glyphset();

    // Host/donor name collisions. All of these are handled here, before we subset
    // font2 to the final glyphset, by driving the subset visitor ourselves so that
    // the preseeded state is in place when the blocks are visited:
    //
    //  * Glyph classes: FEA class definitions are unscoped globals, so if both
    //    fonts define `@FOO` the merged file would define it twice. The donor's is
    //    renamed `@FOO_2` (and its references updated).
    //  * Lookups: under the default "first" policy a lookup which already exists
    //    in the host font wins, so the donor's same-named lookup is dropped, along
    //    with every reference to it; under "both" it is renamed instead, so that
    //    both survive.
    if layout_handling != LayoutHandling::Ignore {
        let (host_lookups, host_classes) = collect_names(&font1);
        if !host_lookups.is_empty() || !host_classes.is_empty() {
            log::debug!(
                "Host font defines lookups {host_lookups:?} and classes {host_classes:?}; \
                 disambiguating the donor's"
            );
            disambiguate_donor_names(
                &mut font2,
                host_lookups,
                host_classes,
                duplicate_lookups,
                &final_glyphset,
            )
            .expect("Failed to disambiguate donor feature names");
        }
    }

    // This performs the layout subsetting. We retain not just the incoming glyphset but also
    // font1's existing glyphset, so that layout rules and kerning pairs which cross-reference a
    // glyph already present in the host font are not stripped out by babelfont (which otherwise
    // has no visibility of the host font's glyphset).
    RetainGlyphs::new(
        final_glyphset
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>(),
    )
    .apply(&mut font2)
    .expect("Failed to retain selected glyphs in font 2");

    if layout_handling != LayoutHandling::Ignore {
        sanity_check_features(&font2);
    }

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

    glyphset_filter.sort_glyphset(&mut font2);
    glyphset_filter.de_encode(&mut font1, &mut font2);

    add_needed_masters(&mut font1, &mut font2, process_avar_mapping)
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
            merge_glyph(
                &mut font1,
                &f1_nonsparse_master_ids,
                g,
                &f2_axes,
                &font2,
                &mapping,
            )
            .expect("Failed to merge glyph");
        }
    }
    assert!(
        sanity_check(&font1),
        "Font failed sanity check after merging glyphs"
    );

    // Handle dotted circle anchors: merge in any anchors from font2's dotted circle glyph
    // that aren't already present (by name) on font1's, rather than letting the glyph merge
    // above simply replace font1's anchors with font2's.
    if let Some((ref orig1, ref orig2)) = dotted_circle_pair {
        merge_dotted_circle_anchors_into(&mut font1, orig1, orig2);
    }

    if layout_handling != LayoutHandling::Ignore {
        sanity_check_features(&font1);
    }
    Ok(font1)
}

/// Gather the names of the lookups and glyph classes defined in a font's feature
/// code. Class names are returned without their leading `@`, matching the
/// subsetter's vocabulary.
fn collect_names(font: &babelfont::Font) -> (HashSet<SmolStr>, HashSet<SmolStr>) {
    let glyph_names = font
        .glyphs
        .iter()
        .map(|g| g.name.as_str())
        .collect::<Vec<_>>();
    let features = font.features.to_fea();
    let mut feature_file = match fea_rs_ast::FeatureFile::new_from_fea(
        &features,
        Some(&glyph_names),
        font.source.clone(),
    ) {
        Ok(feature_file) => feature_file,
        Err(e) => {
            log::warn!("Failed to parse host font features while gathering names: {e}");
            return (HashSet::new(), HashSet::new());
        }
    };
    let mut visitor = NameGathererVisitor::new();
    if let Err(e) = visitor.visit(&mut feature_file) {
        log::warn!("Failed to gather names from host font features: {e}");
        return (HashSet::new(), HashSet::new());
    }
    (visitor.lookup_names, visitor.class_names)
}

/// Rename or drop, in `font2`, anything whose name the host font has already
/// claimed: class definitions get a numeric suffix (with their references
/// updated), and lookups are either dropped or renamed depending on the duplicate
/// policy. Features left empty as a result are culled.
fn disambiguate_donor_names(
    font2: &mut babelfont::Font,
    host_lookups: HashSet<SmolStr>,
    host_classes: HashSet<SmolStr>,
    duplicate_lookups: DuplicateLookupHandling,
    final_glyphset: &[SmolStr],
) -> Result<(), error::FontmergeError> {
    let glyph_names = font2
        .glyphs
        .iter()
        .map(|g| g.name.as_str())
        .collect::<Vec<_>>();
    let features = font2.features.to_fea();
    let mut feature_file =
        fea_rs_ast::FeatureFile::new_from_fea(&features, Some(&glyph_names), font2.source.clone())
            .map_err(|e| {
                error::FontmergeError::Parse(format!("Failed to parse font 2 features: {e}"))
            })?;
    let glyph_set: HashSet<&str> = final_glyphset.iter().map(|g| g.as_str()).collect();
    let mut visitor = SubsetVisitor::new(glyph_set);
    match duplicate_lookups {
        DuplicateLookupHandling::First => visitor.drop_lookups(host_lookups),
        DuplicateLookupHandling::Both => visitor.reserve_lookup_names(host_lookups),
    }
    visitor.reserve_class_names(host_classes);
    visitor.visit(&mut feature_file).map_err(|e| {
        error::FontmergeError::Parse(format!("Failed to disambiguate donor names: {e}"))
    })?;
    font2.features = Features::from_fea(&feature_file.as_fea(""));
    Ok(())
}

/// Find the glyph conventionally used to carry a "dotted circle" mark-attachment
/// demonstration (used by shaping engines to render isolated marks).
fn find_dotted_circle(font: &babelfont::Font) -> Option<babelfont::Glyph> {
    font.glyphs
        .get("dottedCircle")
        .or_else(|| font.glyphs.get("uni25CC"))
        .or_else(|| font.glyphs.iter().find(|g| g.codepoints.contains(&0x25CC)))
        .cloned()
}

/// Union the anchors of `orig1` (font1's dotted circle glyph, pre-merge) and `orig2`
/// (font2's dotted circle glyph, pre-merge) by name, preferring font1's anchor when both
/// have one of the same name, and write the result back onto font1's (now merged) glyph.
fn merge_dotted_circle_anchors_into(
    font1: &mut babelfont::Font,
    orig1: &babelfont::Glyph,
    orig2: &babelfont::Glyph,
) {
    let Some(glyph) = font1.glyphs.get_mut(orig1.name.as_str()) else {
        return;
    };
    for (i, layer) in glyph.layers.iter_mut().enumerate() {
        let mut anchors = orig1
            .layers
            .get(i)
            .map(|l| l.anchors.clone())
            .unwrap_or_default();
        let names: std::collections::HashSet<String> =
            anchors.iter().map(|a| a.name.clone()).collect();
        if let Some(l2) = orig2.layers.get(i) {
            for anchor in &l2.anchors {
                if !names.contains(&anchor.name) {
                    anchors.push(anchor.clone());
                }
            }
        }
        layer.anchors = anchors;
    }
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
    let glyph_map = GlyphMap::new(font.glyphs.iter().map(|g| g.name.as_str()))
        .expect("Failed to create glyph map for sanity check");
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

#[cfg(test)]
pub(crate) fn create_test_font(glyphs: Vec<SmolStr>, feature_code: &str) -> babelfont::Font {
    let mut font = babelfont::Font {
        // Feature parsing/validation needs a base directory to resolve includes against.
        source: Some(std::path::PathBuf::from(".")),
        ..Default::default()
    };
    font.masters.push(babelfont::Master {
        id: "master0".into(),
        ..Default::default()
    });
    for glyph_name in glyphs {
        let mut layer = babelfont::Layer::new(500.0);
        layer.master = babelfont::LayerType::DefaultForMaster("master0".into());
        font.glyphs.push(babelfont::Glyph {
            name: glyph_name,
            layers: vec![layer],
            ..Default::default()
        });
    }
    font.features = babelfont::Features::from_fea(feature_code);
    font
}

#[cfg(test)]
mod fontmerge_tests;
