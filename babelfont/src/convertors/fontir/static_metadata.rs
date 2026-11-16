use std::collections::{HashMap, HashSet};

use super::BabelfontIrSource;
use crate::{Font, MetricType};
use fontdrasil::{orchestration::Work, types::GlyphName};
use fontir::{
    error::{BadSourceKind, Error},
    ir::{
        GlyphOrder, NameBuilder, NameKey, NamedInstance, PostscriptNames,
        PreliminaryGdefCategories, StaticMetadata,
    },
    orchestration::{Context, Flags, WorkId},
};
use write_fonts::{
    tables::{gdef::GlyphClassDef, os2::SelectionFlags},
    types::NameId,
};

fn make_glyph_categories(font: &Font) -> PreliminaryGdefCategories {
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
                    _ => GlyphClassDef::Base, // default to Base
                },
            )
        })
        .collect();
    PreliminaryGdefCategories {
        categories,
        infer_from_anchors: false,
        mark_category_glyphs: Default::default(),
    }
}

/// The legacy family and subfamily names (IDs 1 and 2) a source states, as UFO's
/// `styleMapFamilyName` and `styleMapStyleName` (see `ufo::KEY_STYLE_MAP_*`); a
/// TTF states them too, and its own need not be the four style-linked styles
const KEY_STYLE_MAP_FAMILY_NAME: &str = "ufo.styleMapFamilyName";
const KEY_STYLE_MAP_STYLE_NAME: &str = "ufo.styleMapStyleName";

fn stated(font: &Font, key: &str) -> Option<String> {
    font.format_specific
        .get(key)
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn names(font: &Font, flags: SelectionFlags) -> HashMap<NameKey, String> {
    let mut builder = NameBuilder::default();
    builder.set_version(font.version.0 as i32, font.version.1 as u32);

    for name_id_u32 in 0..=25 {
        let name_id = NameId::from(name_id_u32);
        if let Some(name) = font.names.get(name_id).and_then(|n| {
            n.get_default()
                .or(n.is_single().then(|| n.0.values().next()).flatten())
        }) {
            builder.add(name_id, name.clone());
        }
    }

    let subfamily = stated(font, KEY_STYLE_MAP_STYLE_NAME).unwrap_or_else(|| {
        if flags.contains(SelectionFlags::BOLD | SelectionFlags::ITALIC) {
            "Bold Italic"
        } else if flags.contains(SelectionFlags::BOLD) {
            "Bold"
        } else if flags.contains(SelectionFlags::ITALIC) {
            "Italic"
        } else {
            "Regular"
        }
        .to_string()
    });
    builder.add(NameId::SUBFAMILY_NAME, subfamily);

    // Family name needs to include style, with some mutilation (drop last Regular, Bold, Italic)
    // <https://github.com/googlefonts/glyphsLib/blob/74c63244fdbef1da540d646b0784ae6d2c3ca834/Lib/glyphsLib/builder/names.py#L92>
    let original_family = builder
        .get(NameId::FAMILY_NAME)
        .map(|s| s.to_string())
        .unwrap_or_default();
    let default_style = font
        .default_master()
        .and_then(|f| f.name.get_default())
        .map(|f| f.as_str())
        .unwrap_or("Regular");
    let family = stated(font, KEY_STYLE_MAP_FAMILY_NAME)
        .unwrap_or_else(|| NameBuilder::make_family_name(&original_family, default_style, true));
    builder.add(NameId::FAMILY_NAME, family);

    if let Some(typographic_family) = &builder
        .get(NameId::TYPOGRAPHIC_FAMILY_NAME)
        .or(Some(&original_family))
    {
        builder.add(
            NameId::TYPOGRAPHIC_FAMILY_NAME,
            typographic_family.to_string(),
        );
    }

    if let Some(typographic_subfamily) = &builder
        .get(NameId::TYPOGRAPHIC_SUBFAMILY_NAME)
        .or(Some(default_style))
    {
        builder.add(
            NameId::TYPOGRAPHIC_SUBFAMILY_NAME,
            typographic_subfamily.to_string(),
        );
    }
    let vendor_id = font
        .custom_ot_values
        .os2_vendor_id
        .map(|id| id.to_string())
        .unwrap_or("NONE".to_string());

    let mut names = builder.build(&vendor_id);
    // the compiler leaves out typographic names that repeat IDs 1 and 2, as
    // ufo2ft does; a font that states them keeps them
    for (name_id, stated) in [
        (
            NameId::TYPOGRAPHIC_FAMILY_NAME,
            &font.names.typographic_family,
        ),
        (
            NameId::TYPOGRAPHIC_SUBFAMILY_NAME,
            &font.names.typographic_subfamily,
        ),
    ] {
        if let Some(value) = stated.get_default().filter(|v| !v.is_empty()) {
            if !names.keys().any(|key| key.name_id == name_id) {
                names.insert(NameKey::new(name_id, value), value.clone());
            }
        }
    }
    add_localized_names(font, &mut names);
    names
}

/// The names' other languages, where a name is written as the source states it
/// in its default language: a localized family name is no translation of a
/// family name the compiler derived
fn add_localized_names(font: &Font, names: &mut HashMap<NameKey, String>) {
    let written: HashMap<NameId, String> = names
        .iter()
        .filter(|(key, _)| key.platform_id == 3 && key.lang_id == 0x0409)
        .map(|(key, value)| (key.name_id, value.clone()))
        .collect();
    let default_master_name = font.default_master().map(|m| &m.name);
    for (name_id, written) in written {
        let source = match name_id {
            NameId::TYPOGRAPHIC_FAMILY_NAME if font.names.typographic_family.is_empty() => {
                Some(&font.names.family_name)
            }
            NameId::TYPOGRAPHIC_SUBFAMILY_NAME if font.names.typographic_subfamily.is_empty() => {
                default_master_name
            }
            _ => font.names.get(name_id),
        };
        let Some(source) = source else { continue };
        if source.get_default() != Some(&written) {
            continue;
        }
        for (language, value) in source.0.iter() {
            let Some(lang_id) = crate::names::name_language_to_ot_lang_id(language) else {
                log::warn!("No Windows language ID for name language {language}");
                continue;
            };
            if lang_id == 0x0409 || value.is_empty() {
                continue;
            }
            names.insert(
                NameKey::new_with_lang(name_id, value, lang_id),
                value.clone(),
            );
        }
    }
}

/// post.italicAngle, counter-clockwise: the default master's angle, which is
/// whole degrees and clockwise, or the angle a source states more precisely
/// where it rounds to that
fn italic_angle(font: &Font) -> f64 {
    // negate the italic angle because it's clockwise in Glyphs.app whereas it's
    // counter-clockwise in UFO/OpenType and our GlobalMetrics follow the latter
    // https://github.com/googlefonts/glyphsLib/blob/f162e7/Lib/glyphsLib/builder/masters.py#L36
    let master_angle = font
        .default_master()
        .and_then(|x| x.metrics.get(&MetricType::ItalicAngle))
        .map(|v| -v as f64)
        .unwrap_or(0.0);
    match font.custom_ot_values.post_italic_angle {
        Some(precise) if precise.round() == master_angle => precise,
        _ => master_angle,
    }
}

/// The fsSelection bits a font states, with ITALIC, BOLD and REGULAR from its
/// style where it states none of them (UFO's `openTypeOS2Selection` cannot)
fn selection_flags(font: &Font, italic_angle: f64) -> SelectionFlags {
    let style_bits = SelectionFlags::ITALIC | SelectionFlags::BOLD | SelectionFlags::REGULAR;
    let stated = font
        .custom_ot_values
        .os2_fs_selection
        .map(SelectionFlags::from_bits_truncate)
        .unwrap_or_default();
    let mut flags = stated.difference(style_bits);
    if font.names.wws_family_name.get_default().is_some() {
        flags |= SelectionFlags::WWS;
    }
    if stated.intersects(style_bits) {
        return flags | stated.intersection(style_bits);
    }
    let master_name = font
        .default_master()
        .and_then(|x| x.name.get_default())
        .map(|x| x.to_ascii_lowercase())
        .unwrap_or("regular".to_string());
    // if there is an italic angle we're italic
    // <https://github.com/googlefonts/glyphsLib/blob/74c63244fdbef1da540d646b0784ae6d2c3ca834/Lib/glyphsLib/builder/names.py#L25>
    if italic_angle != 0.0 {
        flags |= SelectionFlags::ITALIC;
    }
    // https://github.com/googlefonts/glyphsLib/blob/42bc1db912fd4b66f130fb3bdc63a0c1e774eb38/Lib/glyphsLib/builder/names.py#L27
    flags |= match master_name.as_str() {
        "italic" => SelectionFlags::ITALIC,
        "bold" => SelectionFlags::BOLD,
        "bold italic" => SelectionFlags::BOLD | SelectionFlags::ITALIC,
        _ => SelectionFlags::empty(),
    };
    if !flags.intersects(SelectionFlags::ITALIC | SelectionFlags::BOLD) {
        flags |= SelectionFlags::REGULAR;
    }
    flags
}

/// OS/2, head and post values the font states, where they differ from what a
/// compiler would choose
fn set_stated_values(font: &Font, misc: &mut fontir::ir::MiscMetadata) {
    let ot = &font.custom_ot_values;
    if ot.os2_fs_type.is_some() {
        misc.fs_type = ot.os2_fs_type;
    }
    if ot.os2_us_weight_class.is_some() {
        misc.us_weight_class = ot.os2_us_weight_class;
    }
    if ot.os2_us_width_class.is_some() {
        misc.us_width_class = ot.os2_us_width_class;
    }
    if let Some(family_class) = ot.os2_family_class {
        misc.family_class = Some(family_class as i16);
    }
    if let Some(panose) = ot.os2_panose {
        misc.panose = Some(panose.into());
    }
    if ot.post_is_fixed_pitch.is_some() {
        misc.is_fixed_pitch = ot.post_is_fixed_pitch;
    }
    if let Some(ppem) = ot.head_lowest_rec_ppem {
        misc.lowest_rec_ppm = ppem;
    }
    if let Some(flags) = ot.head_flags {
        misc.head_flags = write_fonts::tables::head::Flags::from_bits_truncate(flags);
    }
    let bits = |ranges: &[Option<u32>]| -> Option<HashSet<u32>> {
        ranges.iter().any(Option::is_some).then(|| {
            ranges
                .iter()
                .enumerate()
                .flat_map(|(word, range)| {
                    let range = range.unwrap_or(0);
                    (0..32)
                        .filter(move |bit| range & (1 << bit) != 0)
                        .map(move |bit| word as u32 * 32 + bit)
                })
                .collect()
        })
    };
    if let Some(unicode_ranges) = bits(&[
        ot.os2_unicode_range1,
        ot.os2_unicode_range2,
        ot.os2_unicode_range3,
        ot.os2_unicode_range4,
    ]) {
        misc.unicode_range_bits = Some(unicode_ranges);
    }
    if let Some(code_pages) = bits(&[ot.os2_code_page_range1, ot.os2_code_page_range2]) {
        misc.codepage_range_bits = Some(code_pages);
    }
}

#[derive(Debug)]
pub(crate) struct StaticMetadataWork(pub(crate) BabelfontIrSource);

impl Work<Context, WorkId, Error> for StaticMetadataWork {
    fn id(&self) -> WorkId {
        WorkId::StaticMetadata
    }

    fn also_completes(&self) -> Vec<WorkId> {
        vec![
            WorkId::PreliminaryGlyphOrder,
            WorkId::PreliminaryGdefCategories,
        ]
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
        let axes = font.fontdrasil_axes().map_err(|e| {
            Error::BadSource(fontir::error::BadSource::new(
                self.0
                    .font
                    .source
                    .clone()
                    .unwrap_or("unknown source".into()),
                BadSourceKind::Custom(format!("Error converting axes for static metadata: {e}")),
            ))
        })?;
        let named_instances = font
            .instances
            .iter()
            .map(|inst| {
                inst.location.to_user(&axes).map(|loc| NamedInstance {
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
                    location: loc,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let global_locations = font
            .masters
            .iter()
            .map(|m| m.location.to_normalized(&axes))
            .collect::<Result<HashSet<_>, _>>()?;
        let italic_angle = italic_angle(font);
        let selection_flags = selection_flags(font, italic_angle);

        let categories = make_glyph_categories(font);

        // // Only build vertical metrics if at least one glyph defines a vertical
        // // attribute.
        // // https://github.com/googlefonts/glyphsLib/blob/c4db6b98/Lib/glyphsLib/builder/builders.py#L191-L199
        let build_vertical = font
            .glyphs
            .iter()
            .flat_map(|glyph| glyph.layers.iter())
            .any(|layer| layer.vertical_advance.is_some() || layer.vertical_origin.is_some());

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
            italic_angle,
            None, // glyphsapp_number_values,
            build_vertical,
        )
        .map_err(Error::VariationModelError)?;
        context.preliminary_gdef_categories.set(categories);
        static_metadata.misc.selection_flags = selection_flags;
        static_metadata.variations = None;

        if let Some(vendor_id) = font.custom_ot_values.os2_vendor_id {
            if vendor_id != crate::Tag::new(b"    ") {
                static_metadata.misc.vendor_id = vendor_id;
            }
        }
        set_stated_values(font, &mut static_metadata.misc);

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
