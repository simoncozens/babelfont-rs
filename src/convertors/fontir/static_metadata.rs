use std::collections::HashMap;

use super::BabelfontIrSource;
use crate::{Font, MetricType};
use fontdrasil::{orchestration::Work, types::GlyphName};
use fontir::{
    error::Error,
    ir::{
        GdefCategories, GlyphOrder, NameBuilder, NameKey, NamedInstance, PostscriptNames,
        StaticMetadata,
    },
    orchestration::{Context, Flags, WorkId},
};
use write_fonts::{
    tables::{gdef::GlyphClassDef, os2::SelectionFlags},
    types::NameId,
};

fn make_glyph_categories(font: &Font) -> GdefCategories {
    let categories = font
        .glyphs
        .iter()
        .map(|glyph| {
            (
                GlyphName::new(&glyph.name),
                match glyph.category {
                    crate::GlyphCategory::Base => GlyphClassDef::Base,
                    crate::GlyphCategory::Mark => GlyphClassDef::Mark,
                    crate::GlyphCategory::Ligature => GlyphClassDef::Ligature,
                    crate::GlyphCategory::Unknown => GlyphClassDef::Base, // default to Base
                },
            )
        })
        .collect();
    GdefCategories {
        categories,
        prefer_gdef_categories_in_fea: false,
    }
}

fn names(font: &Font, flags: SelectionFlags) -> HashMap<NameKey, String> {
    let mut builder = NameBuilder::default();
    builder.set_version(font.version.0 as i32, font.version.1 as u32);

    for name_id_u32 in 0..=25 {
        let name_id = NameId::from(name_id_u32);
        if let Some(name) = font.names.get(name_id).and_then(|n| n.get_default()) {
            builder.add(name_id, name.clone());
        }
    }

    let subfamily = if flags.contains(SelectionFlags::BOLD | SelectionFlags::ITALIC) {
        "Bold Italic"
    } else if flags.contains(SelectionFlags::BOLD) {
        "Bold"
    } else if flags.contains(SelectionFlags::ITALIC) {
        "Italic"
    } else {
        "Regular"
    };
    builder.add(NameId::SUBFAMILY_NAME, subfamily.to_string());

    // Family name needs to include style, with some mutilation (drop last Regular, Bold, Italic)
    // <https://github.com/googlefonts/glyphsLib/blob/74c63244fdbef1da540d646b0784ae6d2c3ca834/Lib/glyphsLib/builder/names.py#L92>
    let original_family = builder
        .get(NameId::FAMILY_NAME)
        .map(|s| s.to_string())
        .unwrap_or_default();
    let family = NameBuilder::make_family_name(
        &original_family,
        font.default_master()
            .and_then(|f| f.name.get_default())
            .map(|f| f.as_str())
            .unwrap_or("Regular"),
        true,
    );
    builder.add(NameId::FAMILY_NAME, family.clone());

    if let Some(typographic_family) = &builder
        .get(NameId::TYPOGRAPHIC_FAMILY_NAME)
        .or(Some(&original_family))
    {
        builder.add(
            NameId::TYPOGRAPHIC_FAMILY_NAME,
            typographic_family.to_string(),
        );
    }

    if let Some(typographic_subfamily) = &builder.get(NameId::TYPOGRAPHIC_SUBFAMILY_NAME).or(font
        .default_master()
        .and_then(|x| x.name.get_default().map(|x| x.as_str())))
    {
        builder.add(
            NameId::TYPOGRAPHIC_SUBFAMILY_NAME,
            typographic_subfamily.to_string(),
        );
    }

    builder.into_inner()
}

#[derive(Debug)]
pub(crate) struct StaticMetadataWork(pub(crate) BabelfontIrSource);

impl Work<Context, WorkId, Error> for StaticMetadataWork {
    fn id(&self) -> WorkId {
        WorkId::StaticMetadata
    }

    fn also_completes(&self) -> Vec<WorkId> {
        vec![WorkId::PreliminaryGlyphOrder]
    }

    fn exec(&self, context: &Context) -> Result<(), Error> {
        let font = &self.0.font;
        log::debug!(
            "Static metadata for {}",
            font.names
                .family_name
                .get_default()
                .map(|s| s.as_str())
                .unwrap_or("<nameless family>")
        );
        let axes = font.fontdrasil_axes()?;
        let named_instances = font
            .instances
            .iter()
            .map(|inst| NamedInstance {
                name: inst
                    .name
                    .get_default()
                    .map(|x| x.to_string())
                    .unwrap_or("<Unnamed Instance>".to_string()),
                postscript_name: inst
                    .custom_names
                    .postscript_name
                    .get_default()
                    .map(|x| x.to_string()),
                location: inst.location.to_user(&axes),
            })
            .collect();
        let global_locations = font
            .masters
            .iter()
            .map(|m| m.location.to_normalized(&axes))
            .collect();

        // negate the italic angle because it's clockwise in Glyphs.app whereas it's
        // counter-clockwise in UFO/OpenType and our GlobalMetrics follow the latter
        // https://github.com/googlefonts/glyphsLib/blob/f162e7/Lib/glyphsLib/builder/masters.py#L36
        let italic_angle = font
            .default_master()
            .and_then(|x| x.metrics.get(&MetricType::ItalicAngle))
            .map(|v| -v as f32)
            .unwrap_or(0.0);

        let master_name = font
            .default_master()
            .and_then(|x| x.name.get_default())
            .map(|x| x.to_ascii_lowercase())
            .unwrap_or("regular".to_string());

        let mut selection_flags = match false {
            true => SelectionFlags::USE_TYPO_METRICS,
            false => SelectionFlags::empty(),
        } | match font.names.wws_family_name.get_default().is_some() {
            true => SelectionFlags::WWS,
            false => SelectionFlags::empty(),
        } |
        // if there is an italic angle we're italic
        // <https://github.com/googlefonts/glyphsLib/blob/74c63244fdbef1da540d646b0784ae6d2c3ca834/Lib/glyphsLib/builder/names.py#L25>
        match italic_angle {
            0.0 => SelectionFlags::empty(),
            _ => SelectionFlags::ITALIC,
        } |
        // https://github.com/googlefonts/glyphsLib/blob/42bc1db912fd4b66f130fb3bdc63a0c1e774eb38/Lib/glyphsLib/builder/names.py#L27
        match master_name.as_str() {
            "italic" => SelectionFlags::ITALIC,
            "bold" => SelectionFlags::BOLD,
            "bold italic" => SelectionFlags::BOLD | SelectionFlags::ITALIC,
            _ => SelectionFlags::empty(),
        };
        if selection_flags.intersection(SelectionFlags::ITALIC | SelectionFlags::BOLD)
            == SelectionFlags::empty()
        {
            selection_flags |= SelectionFlags::REGULAR;
        }

        let categories = make_glyph_categories(font);

        // // Only build vertical metrics if at least one glyph defines a vertical
        // // attribute.
        // // https://github.com/googlefonts/glyphsLib/blob/c4db6b98/Lib/glyphsLib/builder/builders.py#L191-L199
        // let build_vertical = font
        //     .glyphs
        //     .iter()
        //     .flat_map(|glyph| glyph.layers.iter())
        //     .any(|layer| layer.vert_width.is_some() || layer.vert_origin.is_some());
        let build_vertical = false;

        let dont_use_prod_names = self.0.options.dont_use_production_names;

        let postscript_names =
            if context.flags.contains(Flags::PRODUCTION_NAMES) && !dont_use_prod_names {
                let mut postscript_names = PostscriptNames::default();
                for glyph in font.glyphs.iter() {
                    if let Some(production_name) = glyph.production_name.as_ref() {
                        postscript_names
                            .insert(glyph.name.clone().into(), production_name.clone().into());
                    }
                }
                Some(postscript_names)
            } else {
                None
            };

        let mut static_metadata = StaticMetadata::new(
            font.upm,
            names(font, selection_flags),
            axes.into_inner(),
            named_instances,
            global_locations,
            postscript_names,
            italic_angle.into(),
            categories,
            None,
            build_vertical,
        )
        .map_err(Error::VariationModelError)?;
        static_metadata.misc.selection_flags = selection_flags;
        static_metadata.variations = None;

        // treat "    " (four spaces) as equivalent to no value; it means
        // 'null', per the spec
        // if let Some(vendor_id) = font.vendor_id().filter(|id| *id != "    ") {
        //     static_metadata.misc.vendor_id =
        //         vendor_id.parse().map_err(|cause| Error::InvalidTag {
        //             raw_tag: vendor_id.to_owned(),
        //             cause,
        //         })?;
        // }

        // // Default per <https://github.com/googlefonts/glyphsLib/blob/cb8a4a914b0a33431f0a77f474bf57eec2f19bcc/Lib/glyphsLib/builder/custom_params.py#L1117-L1119>
        // static_metadata.misc.fs_type = font.custom_parameters.fs_type.or(Some(1 << 3));

        // static_metadata.misc.is_fixed_pitch = font.custom_parameters.is_fixed_pitch;

        // static_metadata.misc.unicode_range_bits = font
        //     .custom_parameters
        //     .unicode_range_bits
        //     .as_ref()
        //     .map(|bits| bits.iter().copied().collect());
        // static_metadata.misc.codepage_range_bits = font
        //     .custom_parameters
        //     .codepage_range_bits
        //     .as_ref()
        //     .map(|bits| bits.iter().copied().collect());

        // let default_master = font.default_master();
        // let default_location = font.default_location()?;
        // let default_instance = font
        //     .instances
        //     .iter()
        //     .find(|instance| instance.location == default_location);
        // if let Some(raw_panose) = default_instance
        //     .and_then(|di| di.custom_parameters.panose.as_ref())
        //     .or(default_master.custom_parameters.panose.as_ref())
        //     .or(font.custom_parameters.panose.as_ref())
        // {
        //     let mut bytes = [0u8; 10];
        //     bytes
        //         .iter_mut()
        //         .zip(raw_panose)
        //         .for_each(|(dst, src)| *dst = *src as u8);
        //     static_metadata.misc.panose = Some(bytes.into());
        // }

        static_metadata.misc.version_major = font.version.0 as i32;
        static_metadata.misc.version_minor = font.version.1 as u32;
        // if let Some(lowest_rec_ppm) = font.custom_parameters.lowest_rec_ppem {
        //     static_metadata.misc.lowest_rec_ppm = lowest_rec_ppm as _;
        // }

        static_metadata.misc.created = Some(font.date.to_utc());

        // if let Some(meta_table) = default_instance
        //     .and_then(|di| di.custom_parameters.meta_table.as_ref())
        //     .or(default_master.custom_parameters.meta_table.as_ref())
        //     .or(font.custom_parameters.meta_table.as_ref())
        // {
        //     static_metadata.misc.meta_table = Some(MetaTableValues {
        //         dlng: meta_table.dlng.clone(),
        //         slng: meta_table.slng.clone(),
        //     });
        // }

        // if let Some(gasp) = &font.custom_parameters.gasp_table {
        //     for (max_ppem, behavior) in gasp.iter() {
        //         let Ok(range_max_ppem) = (*max_ppem).try_into() else {
        //             warn!(
        //                 "Invalid gasp entry, rangeMaxPPEM {max_ppem} out of bounds, ignoring range"
        //             );
        //             continue;
        //         };
        //         let range_gasp_behavior = GaspRangeBehavior::from_bits_truncate(*behavior as u16);
        //         if range_gasp_behavior == GaspRangeBehavior::empty() {
        //             warn!("Invalid gasp entry at rangeMaxPPEM {max_ppem}, no behavior bits set by {behavior}, ignoring range");
        //             continue;
        //         }
        //         static_metadata.misc.gasp.push(GaspRange {
        //             range_max_ppem,
        //             range_gasp_behavior,
        //         });
        //     }
        // }

        let glyph_order: GlyphOrder = font
            .glyphs
            .iter()
            .map(|g| GlyphName::new(&g.name))
            .collect();

        context.static_metadata.set(static_metadata);
        context.preliminary_glyph_order.set(glyph_order);
        Ok(())
    }
}
