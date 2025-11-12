use babelfont::filters::{FontFilter as _, RetainGlyphs};
use clap::Parser;
use indexmap::IndexSet;
use indicatif::ProgressIterator;
mod args;
mod designspace;
mod error;
mod glyphset;
mod kerning;
mod layout;
mod merge;

use crate::args::{DuplicateLookupHandling, ExistingGlyphHandling, LayoutHandling};
use crate::designspace::{add_needed_masters, fontdrasil_axes, map_designspaces, sanity_check};
use crate::kerning::merge_kerning;
use crate::layout::lookupgatherer::LookupGathererVisitor;
use crate::layout::subsetter::LayoutSubsetter;
use crate::layout::visitor::LayoutVisitor;
use crate::merge::merge_glyph;
use babelfont::load;
use regex::Regex;

use std::path::PathBuf;
use std::sync::LazyLock;

#[allow(clippy::unwrap_used)] // Static regex is safe to unwrap
static GLYPH_CLASS_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\b(@[^\s,@]+)"#).unwrap());

fn discover_hidden_classes(font: &babelfont::Font) -> IndexSet<String> {
    let mut hidden_classes = IndexSet::new();
    for glyph in font.glyphs.iter() {
        for layer in glyph.layers.iter() {
            for anchor in layer.anchors.iter() {
                // GPOS context is either stored in ["com.schriftgestalt.Glyphs.userData"]["GPOS_Context"]
                // or in ["norad.lib"]["GPOS_Context"] depending on source format.
                let context_root = anchor.format_specific.get("norad.lib").or_else(|| {
                    anchor
                        .format_specific
                        .get("com.schriftgestalt.Glyphs.userData")
                });
                if let Some(context) = context_root
                    .and_then(|v| v.get("GPOS_Context"))
                    .and_then(|v| v.as_str())
                {
                    // parse out anything that looks like a glyph class
                    for cap in GLYPH_CLASS_REGEX.captures_iter(context) {
                        hidden_classes.insert(cap[1].to_string());
                    }
                }
            }
        }
    }
    hidden_classes
}

fn main() {
    let args = args::Args::parse();
    env_logger::Builder::new()
        .filter_level(args.verbosity.into())
        .init();
    log::debug!("Loading font 1");
    let mut font1 = load(&args.font_1).expect("Failed to load font 1");
    log::debug!("Loading font 2");
    let mut font2 = load(&args.font_2).expect("Failed to load font 2");
    let font1_root = PathBuf::from(args.font_1)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    let font2_root = PathBuf::from(args.font_2)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    let mut glyphset_filter = glyphset::GlyphsetFilter::new(
        args.glyph_selection
            .get_include_glyphs()
            .expect("Failed to get include glyphs"),
        args.glyph_selection
            .get_exclude_glyphs()
            .expect("Failed to get exclude glyphs"),
        args.glyph_selection
            .get_codepoints()
            .expect("Failed to get codepoints"),
        &mut font1,
        &font2,
        args.existing_handling,
    );
    glyphset_filter.check_for_presence(&font2);

    let font2_glyphnames = font2
        .glyphs
        .iter()
        .map(|g| g.name.as_str())
        .collect::<Vec<&str>>();
    if args.layout_handling == LayoutHandling::Closure {
        glyphset_filter
            .perform_layout_closure(&font2.features, &font2_glyphnames, &font2_root)
            .expect("Failed to perform layout closure");
    }

    if glyphset_filter.incoming_glyphset.is_empty() {
        log::warn!("No glyphs selected for merging from font 2; exiting");
        return;
    }

    // Babelfont can slim down the font for us
    let mut font2 = font2.clone();
    RetainGlyphs::new(
        glyphset_filter
            .incoming_glyphset
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
    )
    .apply(&mut font2)
    .expect("Failed to retain selected glyphs in font 2");

    if args.layout_handling != LayoutHandling::Ignore {
        let final_glyphset: Vec<String> = glyphset_filter.final_glyphset();
        // Create a layout subsetter
        let pre_existing_lookups = if args.duplicate_lookups == DuplicateLookupHandling::First {
            // Preseed the subsetter with existing lookups from font1
            let font1_glyphnames = font1
                .glyphs
                .iter()
                .map(|g| g.name.as_str())
                .collect::<Vec<&str>>();
            let font1_parse_tree = crate::layout::get_parse_tree(
                &font1.features.to_fea(),
                &font1_glyphnames,
                &font1_root,
            )
            .expect("Failed to get parse tree for font 1");
            let mut visitor = LookupGathererVisitor::new(&font1_parse_tree);
            visitor.visit();
            visitor.lookup_names
        } else {
            IndexSet::new()
        };
        let hidden_classes = discover_hidden_classes(&font2);
        let final_glyphset_refs = final_glyphset
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<&str>>();
        let mut layout_subsetter = LayoutSubsetter::new(
            &font2.features,
            &font2_glyphnames,
            &final_glyphset_refs,
            &hidden_classes,
            &pre_existing_lookups,
            &font2_root,
        );
        let subsetted_features = layout_subsetter
            .subset()
            .expect("Failed to subset layout features");
        font1
            .features
            .prefixes
            .insert("anonymous".into(), subsetted_features.to_fea());
        // merge_features(&mut font1, &subsetted_features);
    }
    // Parse feature file here

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

    add_needed_masters(&mut font1, &font2)
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
        if args.existing_handling == ExistingGlyphHandling::Skip
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

    // Handle dotted circle anchors
    log::info!(
        "Saving merged font to {}",
        args.output.as_deref().unwrap_or("stdout")
    );

    if let Some(output) = &args.output {
        font1.save(output).expect("Failed to save merged font");
    }
}

fn set_layer_locations(glyph_name: &String, font: &mut babelfont::Font) {
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
