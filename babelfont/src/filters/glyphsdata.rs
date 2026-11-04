use std::sync::LazyLock;

use serde::Deserialize;

use crate::filters::FontFilter;

#[derive(Debug, Deserialize)]
pub(crate) struct GlyphInfo {
    /// Other names the glyph went by, comma-separated
    alt_names: Option<String>,
    pub(crate) name: String,
    category: String,
    sub_category: Option<String>,
    // case: Option<String>,
    // direction: Option<String>,
    // script: Option<String>,
    production: Option<String>,
    // alt_names: Vec<String>,
}

const GLYPHS_DATA_STR: &str = include_str!(concat!(env!("OUT_DIR"), "/glyphsdata.json"));
#[allow(clippy::expect_used)]
static GLYPHS_DATA: LazyLock<Vec<GlyphInfo>> =
    LazyLock::new(|| serde_json::from_str(GLYPHS_DATA_STR).expect("Failed to parse glyphs data"));

struct GlyphsDataIndex {
    by_name: std::collections::HashMap<&'static str, &'static GlyphInfo>,
    by_alt_name: std::collections::HashMap<&'static str, &'static GlyphInfo>,
}

static GLYPHS_DATA_INDEX: LazyLock<GlyphsDataIndex> = LazyLock::new(|| {
    let mut index = GlyphsDataIndex {
        by_name: Default::default(),
        by_alt_name: Default::default(),
    };
    for info in GLYPHS_DATA.iter() {
        index.by_name.insert(info.name.as_str(), info);
        let alt_names = info
            .alt_names
            .as_deref()
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|n| !n.is_empty());
        for alt_name in alt_names {
            index.by_alt_name.entry(alt_name).or_insert(info);
        }
    }
    index
});

/// Glyphs.app's data on a glyph by its name or a name it went by
fn named(name: &str) -> Option<&'static GlyphInfo> {
    let index = &*GLYPHS_DATA_INDEX;
    index
        .by_name
        .get(name)
        .or_else(|| index.by_alt_name.get(name))
        .copied()
}

/// A glyph's category and subcategory, as Glyphs and fontc make them up for
/// a name its data does not have (fontc's construct_category): a glyph with
/// a suffix (acutecomb.case) is of the glyph it varies, a ligature
/// (f_f_l, lam_alef-ar) of its parts: a mark if the first is, the first's
/// if the others are marks, else a letter ligature
fn category_of(name: &str) -> Option<(&'static str, Option<&'static str>)> {
    let attributes = |info: &'static GlyphInfo| (info.category.as_str(), info.sub_category.as_deref());
    if name.starts_with('_') {
        return None;
    }
    if let Some(info) = named(name) {
        return Some(attributes(info));
    }
    // the base: with several suffixes, the longest a known glyph has
    let dots: Vec<usize> = name.match_indices('.').map(|(i, _)| i).collect();
    let base = dots
        .iter()
        .skip(1)
        .map(|i| &name[..*i])
        .find(|base| named(base).is_some())
        .or_else(|| name.split('.').next())
        .unwrap_or(name);
    if let Some(info) = named(base) {
        return Some(attributes(info));
    }
    // a ligature: the parts, each with the last's script suffix where a
    // glyph of that name is known
    let (_, last) = base.rsplit_once('_')?;
    let script = last.rsplit_once('-').map(|(_, script)| script);
    let trimmed = match script {
        Some(script) => base.trim_end_matches(script).trim_end_matches('-'),
        None => base,
    };
    let parts: Vec<String> = trimmed
        .split('_')
        .map(|part| match script {
            Some(script) if !part.contains('-') => {
                let with_script = format!("{part}-{script}");
                if named(part).is_some() && named(&with_script).is_none() {
                    part.to_string()
                } else {
                    with_script
                }
            }
            _ => part.to_string(),
        })
        .collect();
    let infos: Vec<Option<&'static GlyphInfo>> = parts.iter().map(|p| named(p)).collect();
    let first = infos.first().copied().flatten()?;
    match first.category.as_str() {
        "Mark" => Some(("Mark", first.sub_category.as_deref())),
        "Letter" => {
            let rest_attach = infos.iter().skip(1).all(|info| {
                info.is_none_or(|info| matches!(info.category.as_str(), "Mark" | "Separator"))
            });
            if rest_attach {
                Some(attributes(first))
            } else {
                Some(("Letter", Some("Ligature")))
            }
        }
        _ => None,
    }
}

/// A filter that adds Glyphs.app glyph metadata to the font
pub struct GlyphsData;

impl FontFilter for GlyphsData {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        log::info!("Applying GlyphsData filter");
        let index = &*GLYPHS_DATA_INDEX;
        for glyph in font.glyphs.iter_mut() {
            let exact = index.by_name.get(glyph.name.as_str()).copied();
            if let Some(production_name) = exact.and_then(|info| info.production.as_ref()) {
                if glyph.production_name.is_none() {
                    glyph.production_name = Some(production_name.into());
                }
            }
            if glyph.category != crate::GlyphCategory::Unknown {
                continue;
            }
            let Some((category, subcategory)) = category_of(&glyph.name) else {
                continue;
            };
            // glyphsLib and fontc (category_for_glyph_preliminary): a mark
            // only if it attaches, and a ligature by its subcategory. A
            // spacing accent or a modifier letter (acute, apostrophemod) is a
            // base
            glyph.category = match (category, subcategory) {
                ("Mark", Some("Nonspacing" | "Spacing Combining")) => crate::GlyphCategory::Mark,
                (_, Some("Ligature")) => crate::GlyphCategory::Ligature,
                ("Mark", _) => crate::GlyphCategory::Base,
                _ => continue,
            };
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(GlyphsData)
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("glyphsdata")
            .long("glyphs-data")
            .help("Add Glyphs.app glyph metadata to the font")
            .action(clap::ArgAction::SetTrue)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{Font, Glyph, GlyphCategory};

    #[test]
    fn test_only_attaching_marks_are_marks() {
        // Glyphs' data has all three as Mark: Modifier, Spacing and
        // Nonspacing. A shaper zeroes a mark's advance, so glyphsLib and fontc
        // take only a nonspacing or spacing combining one for a mark.
        let mut font = Font::new();
        for name in ["apostrophemod", "acute", "acutecomb"] {
            font.glyphs.push(Glyph::new(name));
        }
        GlyphsData.apply(&mut font).unwrap();
        let category = |name: &str| font.glyphs.get(name).unwrap().category.clone();
        assert_eq!(category("apostrophemod"), GlyphCategory::Base);
        assert_eq!(category("acute"), GlyphCategory::Base);
        assert_eq!(category("acutecomb"), GlyphCategory::Mark);
    }

    #[test]
    fn test_a_variant_is_of_its_glyphs_category() {
        // acutecomb.case is a mark as acutecomb is, though Glyphs' data names
        // only acutecomb; its production name is its own to make
        let mut font = Font::new();
        for name in ["acutecomb.case", "dotbelowcomb.case", "acute.case"] {
            font.glyphs.push(Glyph::new(name));
        }
        GlyphsData.apply(&mut font).unwrap();
        let glyph = |name: &str| font.glyphs.get(name).unwrap();
        assert_eq!(glyph("acutecomb.case").category, GlyphCategory::Mark);
        assert_eq!(glyph("dotbelowcomb.case").category, GlyphCategory::Mark);
        // a spacing accent's variant is a base, as the accent is
        assert_eq!(glyph("acute.case").category, GlyphCategory::Base);
        assert_eq!(glyph("acutecomb.case").production_name, None);
    }

    #[test]
    fn test_ligatures_are_ligatures() {
        // as fontc makes up a category for a name Glyphs' data lacks: by the
        // glyph a suffix varies, else by the parts of a ligature
        let mut font = Font::new();
        for name in ["fi", "f_f_l", "f_f.ss01", "A_V", "a_acutecomb", "acutecomb_gravecomb"] {
            font.glyphs.push(Glyph::new(name));
        }
        GlyphsData.apply(&mut font).unwrap();
        let category = |name: &str| font.glyphs.get(name).unwrap().category.clone();
        assert_eq!(category("fi"), GlyphCategory::Ligature);
        assert_eq!(category("f_f_l"), GlyphCategory::Ligature);
        assert_eq!(category("f_f.ss01"), GlyphCategory::Ligature);
        assert_eq!(category("A_V"), GlyphCategory::Ligature);
        // a letter with marks is the letter; marks together a mark
        assert_eq!(category("a_acutecomb"), GlyphCategory::Unknown);
        assert_eq!(category("acutecomb_gravecomb"), GlyphCategory::Mark);
    }
}
