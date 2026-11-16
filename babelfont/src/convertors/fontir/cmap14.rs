use std::collections::{BTreeMap, HashMap};

use write_fonts::{
    from_obj::ToOwnedTable,
    read::{FontRef, TableProvider},
    tables::cmap::{
        Cmap, CmapSubtable, DefaultUvs, EncodingRecord, NonDefaultUvs, PlatformId, UnicodeRange,
        UvsMapping, VariationSelector,
    },
    types::{GlyphId16, Uint24},
    FontBuilder,
};

use crate::{BabelfontError, Font};

/// A selector's codepoints that use the default glyph, and those that name one
type SelectorRecords = (Vec<u32>, Vec<(u32, u16)>);

/// Adds the font's Unicode variation sequences to a compiled binary, as a cmap
/// format 14 subtable (the compiler writes none). A sequence that names the
/// glyph its codepoint maps to anyway uses the default glyph; others name
/// theirs. A sequence whose glyph the binary lacks is left out.
pub(crate) fn insert_variation_sequences(
    binary: &[u8],
    font: &Font,
) -> Result<Vec<u8>, BabelfontError> {
    let fontref = FontRef::new(binary)?;
    let cmap = fontref.cmap()?;
    // the binary's glyphs by name: production names, unless it kept the source's
    let mut glyph_ids: HashMap<&str, GlyphId16> = HashMap::new();
    if let Ok(post) = fontref.post() {
        for gid in 0..fontref.maxp()?.num_glyphs() {
            let gid = GlyphId16::new(gid);
            if let Some(name) = post.glyph_name(gid) {
                glyph_ids.insert(name, gid);
            }
        }
    }
    let glyph_id = |name: &str| -> Option<GlyphId16> {
        let glyph = font.glyphs.get(name);
        glyph
            .and_then(|g| g.production_name.as_deref())
            .and_then(|production| glyph_ids.get(production))
            .or_else(|| glyph_ids.get(name))
            .copied()
    };

    let mut by_selector: BTreeMap<u32, SelectorRecords> = BTreeMap::new();
    for (&(selector, codepoint), glyph) in &font.variation_sequences {
        let Some(gid) = glyph_id(glyph) else {
            log::warn!("Variation sequence {codepoint:04X} {selector:04X}: no glyph {glyph}");
            continue;
        };
        let entry = by_selector.entry(selector).or_default();
        if cmap.map_codepoint(codepoint).map(|g| g.to_u32()) == Some(gid.to_u32()) {
            entry.0.push(codepoint);
        } else {
            entry.1.push((codepoint, gid.to_u16()));
        }
    }
    if by_selector.is_empty() {
        return Ok(binary.to_vec());
    }

    // header, records, and the tables they point to
    let mut length = 10 + 11 * by_selector.len() as u32;
    let mut selectors = vec![];
    for (selector, (mut defaults, mut variants)) in by_selector {
        defaults.sort();
        variants.sort();
        let mut ranges: Vec<UnicodeRange> = vec![];
        let mut range: Option<(u32, u32)> = None;
        for codepoint in defaults {
            range = match range {
                Some((start, count)) if start + count + 1 == codepoint && count < 255 => {
                    Some((start, count + 1))
                }
                Some((start, count)) => {
                    ranges.push(UnicodeRange::new(Uint24::new(start), count as u8));
                    Some((codepoint, 0))
                }
                None => Some((codepoint, 0)),
            };
        }
        if let Some((start, count)) = range {
            ranges.push(UnicodeRange::new(Uint24::new(start), count as u8));
        }
        let default_uvs = (!ranges.is_empty()).then(|| {
            length += 4 + 4 * ranges.len() as u32;
            DefaultUvs::new(ranges.len() as u32, ranges)
        });
        let non_default_uvs = (!variants.is_empty()).then(|| {
            length += 4 + 5 * variants.len() as u32;
            let mappings: Vec<UvsMapping> = variants
                .into_iter()
                .map(|(codepoint, gid)| UvsMapping::new(Uint24::new(codepoint), gid))
                .collect();
            NonDefaultUvs::new(mappings.len() as u32, mappings)
        });
        selectors.push(VariationSelector::new(
            Uint24::new(selector),
            default_uvs,
            non_default_uvs,
        ));
    }

    let mut cmap: Cmap = cmap.to_owned_table();
    cmap.encoding_records
        .retain(|record| !matches!(record.subtable.as_ref(), CmapSubtable::Format14(_)));
    cmap.encoding_records.push(EncodingRecord::new(
        PlatformId::Unicode,
        5,
        CmapSubtable::format_14(length, selectors.len() as u32, selectors),
    ));
    cmap.encoding_records
        .sort_by_key(|record| (record.platform_id as u16, record.encoding_id));

    let mut builder = FontBuilder::new();
    builder
        .add_table(&cmap)
        .map_err(|e| BabelfontError::General(format!("Error adding cmap table: {e:#?}")))?;
    builder.copy_missing_tables(fontref);
    Ok(builder.build())
}
