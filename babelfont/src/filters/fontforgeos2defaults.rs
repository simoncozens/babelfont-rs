use crate::filters::FontFilter;
use crate::{Font, Master, MetricType, Shape};
use std::collections::HashSet;

/// The eight sub/superscript metrics and two strikeout metrics FontForge computes
/// at export, all together, when the source states no `OS2SubXSize`.
const SUBSUPER: [MetricType; 8] = [
    MetricType::SubscriptXSize,
    MetricType::SubscriptYSize,
    MetricType::SubscriptXOffset,
    MetricType::SubscriptYOffset,
    MetricType::SuperscriptXSize,
    MetricType::SuperscriptYSize,
    MetricType::SuperscriptXOffset,
    MetricType::SuperscriptYOffset,
];
const STRIKEOUT: [MetricType; 2] = [MetricType::StrikeoutSize, MetricType::StrikeoutPosition];

#[derive(Default)]
/// A filter that fills in OS/2 values the way FontForge's exporter does: the ten
/// sub/superscript and strikeout metrics of a source that states no `OS2SubXSize`,
/// replacing any of them it does state, and the PANOSE the source does not state.
///
/// A FontForge `.sfd` usually carries no `OS2SubXSize` and friends. FontForge then
/// computes them while writing the binary, so the exported font has values that are
/// nowhere in the source, and a compiler with a different fallback writes different
/// ones.
///
/// FontForge treats the ten values as one group. Reading an `OS2SubXSize:` line is
/// what marks them as stated (`subsuper_set` in `sfd.c`), and when that mark is unset
/// the exporter computes all ten, replacing any of them the source does state. This
/// filter does the same, keyed on the subscript x size.
///
/// The rule is stated in FontForge's own source, `fontforge/tottf.c`,
/// `SFDefaultOS2SubSuper`:
///
/// ```text
/// os2_supysize = os2_subysize = .7*emsize;
/// os2_supxsize = os2_subxsize = .65*emsize;
/// os2_subyoff  = .14*emsize;
/// os2_supyoff  = .48*emsize;
/// os2_supxoff  =  s*os2_supyoff;    /* s = sin(italic_angle) */
/// os2_subxoff  = -s*os2_subyoff;
/// os2_strikeysize = 102*emsize/2048;
/// os2_strikeypos  = 530*emsize/2048;
/// ```
///
/// `emsize` is ascender + descender. Every field is a 16-bit integer, so each value
/// is truncated towards zero when it is stored, and the x offsets are computed from
/// the y offsets already truncated. The two strikeout lines are integer arithmetic
/// in C, so a 1000-unit em gives 49 and 258 rather than 49.8 and 258.8.
///
/// FontForge uses the italic angle as the source states it, a real number, but the
/// SFD reader rounds it to a whole degree, so a fractional angle gives other x
/// offsets: on a 2048-unit em, `ItalicAngle: -12.5` gives 61 and -212 in FontForge,
/// and 64 and -221 here, from -13.
///
/// # Opt-in, because it is a statement about who exported the binary
///
/// A binary not exported by FontForge has different values, and this filter would
/// move a conversion further from it. Styles of one family may have been exported by
/// different tools, so the choice is made per source.
///
/// When the source states `OS2SubXSize`, none of the ten is changed.
///
/// It also fills PANOSE, as `SFDefaultOS2Info` in the same file does.
/// `SFDefaultOS2Simple` starts it at [2, 0, 5, 3, 0 ...]. `OS2WeightCheck` sets byte
/// 2 from the `Weight:` name and then from the PostScript font name, so a match in
/// the font name wins: medi 6; demi, halb, or semi with bold 7; bold, fett or gras 8;
/// heavy 9; black 10; nord 11; thin 2; extra or light 3. The font name also sets byte
/// 0 to 3 when it contains "script" but not "sans", and byte 3 from its width words:
/// ultra or extra with condensed 8; condensed or narrow 6; ultra or extra with
/// expanded 7; expanded 5. When every glyph FontForge would output has the same
/// advance (`CIDOneWidth`, up to tag 20230101), byte 3 is 9 and byte 0 stays 2. All
/// words match without regard to case, anywhere in the name. So `Weight: Medium`
/// with a font name that matches nothing gives [2, 0, 6, 3, 0 ...].
///
/// A PANOSE the source states is never overwritten.
///
/// # Not modelled: a source that does not set `pfmset`
///
/// Any of the lines `PfmFamily:`, `TTFWeight:`, `PfmWeight:`, `TTFWidth:`, `LineGap:`
/// and `VLineGap:` sets FontForge's `pfmset`. Without one, `SFDefaultOS2Info` resets
/// `pfminfo`, FontForge's record of the OS/2 values, before filling it, so the
/// exporter computes PANOSE and the ten values even when the source states them,
/// derives the weight and width classes from the same name words as PANOSE, and sets
/// the line gaps to `rint(.09*emsize)`. This filter keeps the stated values and sets
/// no weight or width class and no line gap. FontForge's Font Info dialog sets
/// `pfmset` whenever it stores PANOSE or the ten, so a stated value is ignored only in
/// a source edited by hand or written by another tool. A font made in FontForge whose
/// OS/2 tab was never opened also lacks `pfmset`, and FontForge then derives its
/// weight and width classes and line gaps as above.
pub struct FontForgeOs2Defaults;

impl FontForgeOs2Defaults {
    /// Create a new FontForgeOs2Defaults filter
    pub fn new() -> Self {
        FontForgeOs2Defaults
    }
}

impl FontFilter for FontForgeOs2Defaults {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let mut filled = 0;
        for master in font.masters.iter_mut() {
            // `subsuper_set` comes from the `OS2SubXSize:` line alone; without it,
            // `SFDefaultOS2SubSuper` writes all ten.
            if master.metrics.contains_key(&MetricType::SubscriptXSize) {
                continue;
            }
            let ascender = master.metrics.get(&MetricType::Ascender).copied();
            let descender = master.metrics.get(&MetricType::Descender).copied();
            let (Some(a), Some(d)) = (ascender, descender) else {
                continue;
            };
            // FontForge's emsize is ascent + descent, and its descent is a positive
            // number below the baseline where babelfont's is negative.
            let em = a - d;
            if em <= 0 {
                continue;
            }
            let em_f = f64::from(em);
            let italic = master
                .metrics
                .get(&MetricType::ItalicAngle)
                .copied()
                .unwrap_or(0);
            // babelfont holds the italic angle in the opposite sense to the `post`
            // table and the SFD, and FontForge's formula is written against its own
            // sense, so the angle is negated here.
            let s = (f64::from(-italic) * std::f64::consts::PI / 180.0).sin();
            // Stored in 16-bit fields before the x offsets are derived from them.
            let subyoff = (0.14 * em_f) as i32;
            let supyoff = (0.48 * em_f) as i32;
            let value = |m: &MetricType| -> i32 {
                match m {
                    MetricType::SubscriptXSize | MetricType::SuperscriptXSize => {
                        (0.65 * em_f) as i32
                    }
                    MetricType::SubscriptYSize | MetricType::SuperscriptYSize => {
                        (0.7 * em_f) as i32
                    }
                    MetricType::SubscriptYOffset => subyoff,
                    MetricType::SuperscriptYOffset => supyoff,
                    MetricType::SubscriptXOffset => (-s * f64::from(subyoff)) as i32,
                    MetricType::SuperscriptXOffset => (s * f64::from(supyoff)) as i32,
                    // Integer arithmetic, as in the C: 102*em/2048, not 0.0498*em.
                    MetricType::StrikeoutSize => (102 * em) / 2048,
                    MetricType::StrikeoutPosition => (530 * em) / 2048,
                    _ => 0,
                }
            };
            for m in SUBSUPER.iter().chain(STRIKEOUT.iter()) {
                master.metrics.insert(m.clone(), value(m));
                filled += 1;
            }
        }
        if font.custom_ot_values.os2_panose.is_none() {
            font.custom_ot_values.os2_panose = Some(default_panose(font));
            filled += 1;
        }
        log::info!(
            "Filled {filled} OS/2 value(s) using FontForge's SFDefaultOS2SubSuper and \
             SFDefaultOS2Info rules"
        );
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(FontForgeOs2Defaults::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("fontforgeos2defaults")
            .long("fontforge-os2-defaults")
            .help(
                "Compute all ten OS/2 sub/superscript and strikeout metrics of a FontForge \
                 source that states no OS2SubXSize, replacing any it states, and fill the \
                 PANOSE it does not state, using the rules FontForge's exporter uses \
                 (SFDefaultOS2SubSuper and SFDefaultOS2Info in tottf.c). Use only to reproduce \
                 a binary FontForge exported; a binary built by another compiler carries \
                 different values",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

/// The PANOSE `SFDefaultOS2Info` computes for a font that states none.
fn default_panose(font: &Font) -> [u8; 10] {
    let weight = font
        .format_specific
        .get("postscript_weight_name")
        .and_then(|v| v.as_str())
        .map(str::to_ascii_lowercase);
    let font_name = font
        .names
        .postscript_name
        .get_default()
        .map(|n| n.to_ascii_lowercase())
        .unwrap_or_default();
    // `samewid > 0`: a monospaced font is never marked as a script.
    let monospaced = font
        .default_master()
        .or(font.masters.first())
        .and_then(|master| one_width(font, master))
        .is_some_and(|width| width > 0);
    let mut panose = [2, 0, 5, 3, 0, 0, 0, 0, 0, 0];
    if !monospaced && !font_name.contains("sans") && font_name.contains("script") {
        panose[0] = 3;
    }
    if let Some(weight) = weight {
        panose[2] = weight_check(&weight, panose[2]);
    }
    panose[2] = weight_check(&font_name, panose[2]);
    if let Some(width) = width_check(&font_name) {
        panose[3] = width;
    }
    if monospaced {
        panose[3] = 9;
    }
    panose
}

/// `OS2WeightCheck`: PANOSE byte 2 from a lowercased weight or font name, or
/// `current` when no word matches.
fn weight_check(name: &str, current: u8) -> u8 {
    let has = |word: &str| name.contains(word);
    if has("medi") {
        6
    } else if has("demi") || has("halb") || (has("semi") && has("bold")) {
        7
    } else if has("bold") || has("fett") || has("gras") {
        8
    } else if has("heavy") {
        9
    } else if has("black") {
        10
    } else if has("nord") {
        11
    } else if has("thin") {
        2
    } else if has("extra") || has("light") {
        3
    } else {
        current
    }
}

/// PANOSE byte 3 from the width words of a lowercased font name, as
/// `SFDefaultOS2Info` sets it.
fn width_check(name: &str) -> Option<u8> {
    let has = |word: &str| name.contains(word);
    if (has("ultra") || has("extra")) && has("condensed") {
        Some(8)
    } else if has("condensed") || has("narrow") {
        Some(6)
    } else if (has("ultra") || has("extra")) && has("expanded") {
        Some(7)
    } else if has("expanded") {
        Some(5)
    } else {
        None
    }
}

/// `CIDOneWidth` in FontForge's `splinesave.c` up to tag 20230101: the advance shared
/// by every glyph FontForge would output, leaving out `.null`, `nonmarkingreturn` and
/// a `.notdef` with no contours. `None` when two advances differ or no glyph counts.
/// From tag 20251009 on, `SFOneWidth` also leaves out zero-width glyphs.
fn one_width(font: &Font, master: &Master) -> Option<i32> {
    let referenced: HashSet<&str> = font
        .glyphs
        .iter()
        .flat_map(|glyph| glyph.layers.iter())
        .flat_map(|layer| layer.shapes.iter())
        .filter_map(|shape| match shape {
            Shape::Component(component) => Some(component.reference.as_str()),
            Shape::Path(_) => None,
        })
        .collect();
    let mut width = None;
    for glyph in font.glyphs.iter() {
        let Some(layer) = font.master_layer_for(&glyph.name, master) else {
            continue;
        };
        let has_contours = layer.shapes.iter().any(|s| matches!(s, Shape::Path(_)));
        match glyph.name.as_str() {
            ".null" | "nonmarkingreturn" => continue,
            ".notdef" if !has_contours => continue,
            _ => {}
        }
        // `SCWorthOutputting`: it draws something, its width was set, it carries an
        // anchor or another glyph uses it as a component.
        let width_set = glyph
            .format_specific
            .get("sfd.width_set")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let worth_outputting = draws_something(font, master, &glyph.name, 0)
            || width_set
            || !layer.anchors.is_empty()
            || referenced.contains(glyph.name.as_str());
        if !worth_outputting {
            continue;
        }
        let advance = layer.width.round() as i32;
        match width {
            None => width = Some(advance),
            Some(w) if w != advance => return None,
            Some(_) => {}
        }
    }
    width
}

/// `SCDrawsSomething`: the glyph has a contour, or a component that brings one.
fn draws_something(font: &Font, master: &Master, name: &str, depth: usize) -> bool {
    const MAX_REFERENCE_DEPTH: usize = 8;
    let Some(layer) = font.master_layer_for(name, master) else {
        return false;
    };
    layer.shapes.iter().any(|shape| match shape {
        Shape::Path(_) => true,
        Shape::Component(component) => {
            depth < MAX_REFERENCE_DEPTH
                && draws_something(font, master, &component.reference, depth + 1)
        }
    })
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Glyph, Layer, LayerType, Node, NodeType, Path};

    fn font_with(ascender: i32, descender: i32, italic: Option<i32>) -> Font {
        let mut m = Master::default();
        m.metrics.insert(MetricType::Ascender, ascender);
        m.metrics.insert(MetricType::Descender, descender);
        if let Some(i) = italic {
            m.metrics.insert(MetricType::ItalicAngle, i);
        }
        let mut f = Font::new();
        f.masters.push(m);
        f
    }

    fn got(f: &Font, m: MetricType) -> Option<i32> {
        f.masters[0].metrics.get(&m).copied()
    }

    #[test]
    fn test_matches_fontforge_on_a_2048_em() {
        // Ascender 1638, descender -410: a 2048-unit em.
        let mut f = font_with(1638, -410, None);
        FontForgeOs2Defaults::new().apply(&mut f).unwrap();
        assert_eq!(got(&f, MetricType::SubscriptXSize), Some(1331));
        assert_eq!(got(&f, MetricType::SubscriptYSize), Some(1433));
        assert_eq!(got(&f, MetricType::SubscriptYOffset), Some(286));
        assert_eq!(got(&f, MetricType::SuperscriptYSize), Some(1433));
        assert_eq!(got(&f, MetricType::SuperscriptYOffset), Some(983));
        assert_eq!(got(&f, MetricType::StrikeoutSize), Some(102));
        assert_eq!(got(&f, MetricType::StrikeoutPosition), Some(530));
    }

    #[test]
    fn test_strikeout_is_integer_arithmetic() {
        // 102*1000/2048 is 49.8. FontForge does this in C integer arithmetic and
        // gets 49; computing 0.0498*em and rounding would give 50.
        let mut f = font_with(800, -200, None);
        FontForgeOs2Defaults::new().apply(&mut f).unwrap();
        assert_eq!(got(&f, MetricType::StrikeoutSize), Some(49));
        assert_eq!(got(&f, MetricType::StrikeoutPosition), Some(258));
    }

    #[test]
    fn test_italic_angle_slants_the_offsets() {
        // supxoff = s*supyoff, subxoff = -s*subyoff with s = sin(italic_angle). A
        // roman gets zero for both; an italic does not.
        let mut roman = font_with(1638, -410, None);
        FontForgeOs2Defaults::new().apply(&mut roman).unwrap();
        assert_eq!(got(&roman, MetricType::SuperscriptXOffset), Some(0));
        assert_eq!(got(&roman, MetricType::SubscriptXOffset), Some(0));

        // post.italicAngle -12 gives subscriptXOffset +59 and superscriptXOffset
        // -204. babelfont's internal angle has the opposite sign to post's, so that
        // is +12 here.
        let mut italic = font_with(1638, -410, Some(12));
        FontForgeOs2Defaults::new().apply(&mut italic).unwrap();
        assert_eq!(got(&italic, MetricType::SubscriptXOffset), Some(59));
        assert_eq!(got(&italic, MetricType::SuperscriptXOffset), Some(-204));
    }

    #[test]
    fn test_the_x_offsets_come_from_the_truncated_y_offsets() {
        // A 2048 em stores subyoff 286 and supyoff 983, not 286.72 and 983.04.
        // sin(20 degrees) * 286 is 97.8, which truncates to 97; from 286.72 it would
        // be 98.06 and give 98.
        let mut f = font_with(1638, -410, Some(20));
        FontForgeOs2Defaults::new().apply(&mut f).unwrap();
        assert_eq!(got(&f, MetricType::SubscriptXOffset), Some(97));
        assert_eq!(got(&f, MetricType::SuperscriptXOffset), Some(-336));
    }

    #[test]
    fn test_a_stated_subscript_x_size_keeps_all_ten() {
        let mut f = font_with(1638, -410, None);
        f.masters[0].metrics.insert(MetricType::SubscriptXSize, 1300);
        FontForgeOs2Defaults::new().apply(&mut f).unwrap();
        assert_eq!(got(&f, MetricType::SubscriptXSize), Some(1300));
        assert_eq!(got(&f, MetricType::SubscriptYSize), None);
        assert_eq!(got(&f, MetricType::StrikeoutPosition), None);
    }

    fn panose_of(weight: Option<&str>, font_name: &str) -> [u8; 10] {
        let mut f = font_with(1638, -410, None);
        if let Some(w) = weight {
            f.format_specific.insert(
                "postscript_weight_name".to_string(),
                serde_json::Value::String(w.to_string()),
            );
        }
        f.names.postscript_name.set_default(font_name.to_string());
        FontForgeOs2Defaults::new().apply(&mut f).unwrap();
        f.custom_ot_values.os2_panose.unwrap()
    }

    /// A glyph with one triangular contour and the given advance.
    fn add_glyph(f: &mut Font, name: &str, advance: f32, contour: bool) {
        let mut layer = Layer::new(advance);
        layer.master = LayerType::DefaultForMaster(f.masters[0].id.clone());
        if contour {
            let node = |x: f64, y: f64| Node {
                x,
                y,
                nodetype: NodeType::Line,
                ..Default::default()
            };
            layer.shapes.push(Shape::Path(Path {
                nodes: vec![node(0.0, 0.0), node(100.0, 0.0), node(50.0, 100.0)],
                closed: true,
                ..Default::default()
            }));
        }
        let mut glyph = Glyph::new(name);
        glyph.layers.push(layer);
        f.glyphs.push(glyph);
    }

    #[test]
    fn test_panose_comes_from_the_weight_name() {
        // OS2WeightCheck reads sf->weight, the SFD's `Weight:` line:
        // Weight: Medium gives byte 2 = 6.
        let mut f = font_with(1638, -410, None);
        f.format_specific.insert(
            "postscript_weight_name".to_string(),
            serde_json::Value::String("Medium".to_string()),
        );
        FontForgeOs2Defaults::new().apply(&mut f).unwrap();
        assert_eq!(f.custom_ot_values.os2_panose, Some([2, 0, 6, 3, 0, 0, 0, 0, 0, 0]));

        // Nothing recognisable in the weight name falls back to 5.
        let mut plain = font_with(1638, -410, None);
        FontForgeOs2Defaults::new().apply(&mut plain).unwrap();
        assert_eq!(plain.custom_ot_values.os2_panose, Some([2, 0, 5, 3, 0, 0, 0, 0, 0, 0]));
    }

    #[test]
    fn test_light_gives_3() {
        // "extra" or "light" gives 3; the later "light" branch of OS2WeightCheck is
        // never reached.
        assert_eq!(panose_of(Some("Light"), "Foo"), [2, 0, 3, 3, 0, 0, 0, 0, 0, 0]);
        assert_eq!(panose_of(Some("ExtraLight"), "Foo")[2], 3);
        // "bold" is tested before "extra".
        assert_eq!(panose_of(Some("ExtraBold"), "Foo")[2], 8);
    }

    #[test]
    fn test_semi_with_bold_gives_7() {
        assert_eq!(panose_of(Some("SemiBold"), "Foo")[2], 7);
        assert_eq!(panose_of(Some("DemiBold"), "Foo")[2], 7);
    }

    #[test]
    fn test_a_match_in_the_font_name_wins_over_the_weight() {
        assert_eq!(panose_of(Some("Regular"), "Foo-SemiBold")[2], 7);
        assert_eq!(panose_of(Some("Bold"), "Foo-Light")[2], 3);
        // With no match in the font name, the weight's value stands.
        assert_eq!(panose_of(Some("Bold"), "Foo-Regular")[2], 8);
        // With no Weight: line, the font name alone decides.
        assert_eq!(panose_of(None, "Foo-Black")[2], 10);
    }

    #[test]
    fn test_a_script_font_name_sets_byte_0() {
        assert_eq!(panose_of(None, "FooScript")[0], 3);
        // ...unless it also says "sans".
        assert_eq!(panose_of(None, "FooSansScript")[0], 2);
    }

    #[test]
    fn test_width_words_in_the_font_name_set_byte_3() {
        assert_eq!(panose_of(None, "Foo-Condensed")[3], 6);
        assert_eq!(panose_of(None, "Foo-SemiCondensed")[3], 6);
        assert_eq!(panose_of(None, "Foo-Narrow")[3], 6);
        assert_eq!(panose_of(None, "Foo-ExtraCondensed")[3], 8);
        assert_eq!(panose_of(None, "Foo-UltraCondensed")[3], 8);
        assert_eq!(panose_of(None, "Foo-SemiExpanded")[3], 5);
        assert_eq!(panose_of(None, "Foo-Expanded")[3], 5);
        assert_eq!(panose_of(None, "Foo-UltraExpanded")[3], 7);
    }

    #[test]
    fn test_weight_script_and_width_combine() {
        assert_eq!(
            panose_of(Some("Light"), "FooScript-SemiBoldCondensed"),
            [3, 0, 7, 6, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn test_one_advance_for_every_glyph_sets_byte_3_to_9() {
        let mut f = font_with(1638, -410, None);
        f.names.postscript_name.set_default("FooScript".to_string());
        add_glyph(&mut f, "a", 600.0, true);
        add_glyph(&mut f, "b", 600.0, true);
        // A .notdef with no contours does not count.
        add_glyph(&mut f, ".notdef", 1000.0, false);
        FontForgeOs2Defaults::new().apply(&mut f).unwrap();
        // Monospaced: byte 3 is 9, and "script" does not set byte 0.
        assert_eq!(f.custom_ot_values.os2_panose, Some([2, 0, 5, 9, 0, 0, 0, 0, 0, 0]));

        let mut f = font_with(1638, -410, None);
        f.names.postscript_name.set_default("FooScript".to_string());
        add_glyph(&mut f, "a", 600.0, true);
        add_glyph(&mut f, "b", 500.0, true);
        FontForgeOs2Defaults::new().apply(&mut f).unwrap();
        assert_eq!(f.custom_ot_values.os2_panose, Some([3, 0, 5, 3, 0, 0, 0, 0, 0, 0]));
    }

    #[test]
    fn test_glyphs_left_out_of_the_one_advance_check() {
        let mut f = font_with(1638, -410, None);
        add_glyph(&mut f, "a", 600.0, true);
        add_glyph(&mut f, "b", 600.0, true);
        // Left out by name, although their widths were set: .null and
        // nonmarkingreturn always, .notdef when it has no contours.
        for name in [".notdef", ".null", "nonmarkingreturn"] {
            add_glyph(&mut f, name, 1000.0, false);
            f.glyphs
                .get_mut(name)
                .unwrap()
                .format_specific
                .insert("sfd.width_set".to_string(), serde_json::Value::Bool(true));
        }
        // Left out by SCWorthOutputting: no contour, no set width, no anchor, and no
        // glyph uses it as a component.
        add_glyph(&mut f, "blank", 300.0, false);
        FontForgeOs2Defaults::new().apply(&mut f).unwrap();
        assert_eq!(f.custom_ot_values.os2_panose.unwrap()[3], 9);
    }

    #[test]
    fn test_a_panose_the_source_states_is_never_overwritten() {
        let mut f = font_with(1638, -410, None);
        f.custom_ot_values.os2_panose = Some([3, 1, 4, 1, 5, 9, 2, 6, 5, 3]);
        FontForgeOs2Defaults::new().apply(&mut f).unwrap();
        assert_eq!(f.custom_ot_values.os2_panose, Some([3, 1, 4, 1, 5, 9, 2, 6, 5, 3]));
    }

    #[test]
    fn test_without_a_subscript_x_size_all_ten_are_computed() {
        // A strikeout size stated without OS2SubXSize does not set subsuper_set, so
        // the exporter replaces it with the computed one.
        let mut f = font_with(1638, -410, None);
        f.masters[0].metrics.insert(MetricType::StrikeoutSize, 77);
        FontForgeOs2Defaults::new().apply(&mut f).unwrap();
        assert_eq!(got(&f, MetricType::StrikeoutSize), Some(102));
        assert_eq!(got(&f, MetricType::StrikeoutPosition), Some(530));
        assert_eq!(got(&f, MetricType::SubscriptXSize), Some(1331));
    }
}
