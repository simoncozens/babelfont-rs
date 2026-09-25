#![allow(clippy::expect_used, clippy::unwrap_used)]
use std::fs;

use rstest::rstest;
use similar::TextDiff;

use super::emit::to_str;
use super::*;

#[test]
fn test_blank_vendor_is_preserved() {
    // A blank vendor is a deliberate value; dropping it made the downstream
    // default substitute the literal string "NONE".
    let sfd = "SplineFontDB: 3.0\nFontName: T\nAscent: 800\nDescent: 200\n\
                   OS2Vendor: '    '\nBeginChars: 1 1\nStartChar: .notdef\n\
                   Encoding: 0 -1 0\nWidth: 500\nEndChar\nEndChars\nEndSplineFont\n";
    let font = load_str(sfd).expect("blank-vendor SFD should load");
    assert_eq!(
        font.custom_ot_values.os2_vendor_id.map(|t| t.to_string()),
        Some("    ".to_string()),
        "a blank vendor must survive as four spaces, not be dropped"
    );
}

#[test]
fn test_nul_padded_vendor_is_repadded_with_spaces() {
    // FontForge pads a short vendor with NULs inside the quotes; a NUL is
    // not legal in an OpenType tag.
    let sfd = "SplineFontDB: 3.0\nFontName: T\nAscent: 800\nDescent: 200\n\
                   OS2Vendor: 'ltt\u{0}'\nBeginChars: 1 1\nStartChar: .notdef\n\
                   Encoding: 0 -1 0\nWidth: 500\nEndChar\nEndChars\nEndSplineFont\n";
    let font = load_str(sfd).expect("NUL-padded vendor SFD should load");
    assert_eq!(
        font.custom_ot_values.os2_vendor_id.map(|t| t.to_string()),
        Some("ltt ".to_string()),
        "a NUL-padded vendor must be re-padded with spaces"
    );
}

#[test]
fn test_converting_twice_gives_the_same_ids() {
    let sfd = "SplineFontDB: 3.0\nFontName: T\nAscent: 800\nDescent: 200\n\
                   BeginChars: 1 1\nStartChar: .notdef\nEncoding: 0 -1 0\n\
                   Width: 500\nEndChar\nEndChars\nEndSplineFont\n";
    let a = load_str(sfd).expect("load");
    let b = load_str(sfd).expect("load");
    assert_eq!(a.masters[0].id, b.masters[0].id, "master id must be stable");
}

#[test]
fn test_italic_angle_sign_is_converted() {
    // FontForge uses the OpenType convention (negative = right-leaning);
    // Glyphs uses the opposite, and the compiler negates on the way out.
    // Without the conversion here the two negations never cancel and every
    // converted italic is back-slanted.
    let sfd = |angle: &str| {
        format!(
            "SplineFontDB: 3.0\nFontName: T\nAscent: 800\nDescent: 200\n\
                 ItalicAngle: {angle}\nBeginChars: 1 1\nStartChar: .notdef\n\
                 Encoding: 0 -1 0\nWidth: 500\nEndChar\nEndChars\nEndSplineFont\n"
        )
    };
    let angle_of = |src: String| {
        load_str(&src).expect("SFD should load").masters[0]
            .metrics
            .get(&MetricType::ItalicAngle)
            .copied()
    };

    // A right-leaning italic: -12 in the SFD becomes +12 for Glyphs.
    assert_eq!(angle_of(sfd("-12")), Some(12));
    // And back the other way.
    assert_eq!(angle_of(sfd("12")), Some(-12));
    // Upright stays upright, with no negative zero.
    assert_eq!(angle_of(sfd("0")), Some(0));
    // A fractional angle must survive: an integer parse would drop it.
    assert_eq!(angle_of(sfd("-12.4")), Some(12));
    assert_eq!(angle_of(sfd("-12.6")), Some(13));
}

#[test]
fn test_italic_angle_survives_an_sfd_roundtrip_as_text() {
    // The generic round-trip test re-parses the emitted SFD and compares
    // model fields, so a sign flip on BOTH sides cancels out and goes
    // unnoticed. Compare the emitted text instead.
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/fontforge/AmbrosiaItalic.sfd");
    let data = String::from_utf8_lossy(&fs::read(&path).expect("Missing SFD")).into_owned();
    let original = data
        .lines()
        .find(|l| l.starts_with("ItalicAngle:"))
        .expect("this fixture must declare an ItalicAngle");
    assert_eq!(original, "ItalicAngle: -10", "fixture changed");

    let font = load_str(&data).expect("Failed to load SFD");
    // Stored in the Glyphs convention, i.e. negated.
    assert_eq!(
        font.masters[0]
            .metrics
            .get(&MetricType::ItalicAngle)
            .copied(),
        Some(10)
    );

    let emitted = to_str(&font).expect("Failed to emit SFD");
    let emitted_line = emitted
        .lines()
        .find(|l| l.starts_with("ItalicAngle:"))
        .expect("emitted SFD lost its ItalicAngle");
    assert_eq!(
        emitted_line, original,
        "an SFD -> SFD round trip must not flip the italic angle"
    );
}

#[test]
fn test_copyright_line_break_escapes_are_decoded_and_survive_a_roundtrip() {
    // FontForge stores Copyright on one SFD line with "\n" escapes and
    // decodes them on export (devonshire: the SFD says
    // `Reserved\nFont Name`, the shipped binary has a real line break).
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/fontforge/AmbrosiaItalic.sfd");
    let data = String::from_utf8_lossy(&fs::read(&path).expect("Missing SFD")).into_owned();
    let escaped =
        "Copyright: (c) 2011 Someone, with Reserved\\nFont Name \"Ambrosia\" and a C:\\\\path";
    let data = data.replace("Copyright: Generated by Fontographer 3.5", escaped);

    let font = load_str(&data).expect("Failed to load SFD");
    assert_eq!(
        font.names.copyright.get_default().map(String::as_str),
        Some("(c) 2011 Someone, with Reserved\nFont Name \"Ambrosia\" and a C:\\path")
    );

    let emitted = to_str(&font).expect("Failed to emit SFD");
    let emitted_line = emitted
        .lines()
        .find(|l| l.starts_with("Copyright:"))
        .expect("emitted SFD lost its Copyright");
    assert_eq!(emitted_line, escaped);
}

#[test]
fn test_weight_suffix_of_family_name() {
    let split = |family: &str, weight: &str| {
        let mut parser = SfdParser::new(PathBuf::from("test.sfd"));
        parser.font.names.family_name = family.into();
        if !weight.is_empty() {
            parser.font.format_specific.insert(
                "postscript_weight_name".to_string(),
                serde_json::Value::String(weight.to_string()),
            );
        }
        parser.weight_suffix_of_family_name()
    };

    // The case this exists for.
    assert_eq!(
        split("Elsie Black", "Black"),
        Some(("Elsie".to_string(), "Black".to_string()))
    );
    // The family name spaces the weight, the weight itself does not.
    assert_eq!(
        split("Elsie Swash Caps Black", "Black"),
        Some(("Elsie Swash Caps".to_string(), "Black".to_string()))
    );

    // Weights that mean Regular are not a suffix worth splitting; almost
    // every file in a FontForge corpus says "Book".
    assert_eq!(split("Fjord One", "Book"), None);
    assert_eq!(split("Anything", "Regular"), None);
    assert_eq!(split("Anything", "Medium"), None);
    assert_eq!(split("Anything", ""), None);

    // The weight must actually end the family name.
    assert_eq!(split("Elsie", "Black"), None);
    assert_eq!(split("Black Ops One", "Black"), None);

    // Nothing left over is not a split -- a family really called "Black"
    // keeps its name.
    assert_eq!(split("Black", "Black"), None);
    assert_eq!(split("  Black  ", "Black"), None);
}

#[test]
fn test_space_before_italic() {
    // The case this exists for: a run-together compound style.
    assert_eq!(space_before_italic("BoldItalic"), "Bold Italic");
    assert_eq!(space_before_italic("LightItalic"), "Light Italic");

    // A bare slope has no weight to separate it from.
    assert_eq!(space_before_italic("Italic"), "Italic");

    // Already spelled correctly -- must not gain a second space.
    assert_eq!(space_before_italic("Bold Italic"), "Bold Italic");

    // Weight names are spelled without a space by the same convention, so
    // nothing that is not a slope gets split.
    assert_eq!(space_before_italic("SemiBold"), "SemiBold");
    assert_eq!(space_before_italic("Regular"), "Regular");
    assert_eq!(space_before_italic("Bold"), "Bold");

    // "Italic" inside a word is not a suffix and is left alone.
    assert_eq!(space_before_italic("Italiano"), "Italiano");
}

#[rstest]
fn test_roundtrip(#[files("resources/fontforge/*.sfd")] path: PathBuf) {
    let data = String::from_utf8_lossy(&fs::read(&path).expect("Failed to read SFD file bytes"))
        .into_owned();
    let font = load_str(&data).expect("Failed to load SFD font");
    let output = to_str(&font).expect("Failed to convert font back to SFD");

    // Re-parse the generated SFD and compare core model fields.
    let reparsed = load_str(&output).expect("Failed to reparse emitted SFD");

    assert_eq!(
        reparsed.glyphs.len(),
        font.glyphs.len(),
        "glyph count changed"
    );
    assert_eq!(
        reparsed
            .glyphs
            .iter()
            .map(|g| g.name.to_string())
            .collect::<Vec<_>>(),
        font.glyphs
            .iter()
            .map(|g| g.name.to_string())
            .collect::<Vec<_>>(),
        "glyph order changed"
    );

    // Check a few key fields so regressions are surfaced early while
    // acknowledging that this emitter currently serializes only a subset of SFD.
    assert_eq!(
        reparsed
            .names
            .postscript_name
            .get_default()
            .map(|s| s.as_str()),
        font.names.postscript_name.get_default().map(|s| s.as_str())
    );
    assert_eq!(reparsed.masters.len(), font.masters.len());

    // Now do a full diff to see what we're missing
    if output != data && data.split("\n").count() < 1000 {
        let diff = TextDiff::from_lines(&data, &output)
            .unified_diff()
            .context_radius(5)
            .header("Original SFD", "Re-emitted SFD")
            .to_string();
        println!("{}", diff);
        panic!("Roundtrip SFD did not match original");
    }
}

#[test]
fn test_blank_script_tag_becomes_dflt() {
    // FontForge writes a blank script tag for lookups with no script;
    // it must become DFLT/dflt or the emitted languagesystem/script
    // FEA statements are invalid.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Ascent: 800\n",
        "Descent: 200\n",
        "LayerCount: 2\n",
        "Layer: 0 0 \"Back\" 1\n",
        "Layer: 1 0 \"Fore\" 0\n",
        "Lookup: 1 0 0 \"t\" {\"t-1\"} [ 'titl' ('    ' <'dflt' > ) ]\n",
        "BeginChars: 2 2\n",
        "StartChar: A\n",
        "Encoding: 65 65 0\n",
        "Width: 600\n",
        "Substitution2: \"t-1\" A.titl\n",
        "Fore\n",
        "EndChar\n",
        "StartChar: A.titl\n",
        "Encoding: -1 -1 1\n",
        "Width: 600\n",
        "Fore\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );
    let font = load_str(data).expect("Failed to parse blank-script SFD");
    let fea = font.features.to_fea();
    assert!(fea.contains("languagesystem DFLT dflt"), "{}", fea);
    assert!(!fea.contains("languagesystem  "), "{}", fea);
}

#[test]
fn test_quoted_glyph_name_is_decoded_and_trimmed() {
    // A quoted name is modified UTF-7, and FontForge can leave whitespace after
    // the closing quote:
    //   StartChar: "C+AJIA-rculo0"<space>
    // names the glyph `C`, U+0092, `rculo0`.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Ascent: 800\n",
        "Descent: 200\n",
        "LayerCount: 2\n",
        "Layer: 0 0 \"Back\" 1\n",
        "Layer: 1 0 \"Fore\" 0\n",
        "BeginChars: 2 2\n",
        "StartChar: \"C+AJIA-rculo0\" \n",
        "Encoding: 57351 57351 0\n",
        "Width: 600\n",
        "Fore\n",
        "EndChar\n",
        "StartChar: A\n",
        "Encoding: 65 65 1\n",
        "Width: 600\n",
        "Fore\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );
    let font = load_str(data).expect("Failed to parse quoted-name SFD");
    let names: Vec<&str> = font.glyphs.0.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, vec!["C\u{92}rculo0", "A"]);
}

#[test]
fn test_unquoted_glyph_name_is_literal() {
    // FontForge writes a name of printable ASCII unquoted and reads it back as
    // it stands, so a `+` in it is not a modified-UTF-7 shift.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Ascent: 800\n",
        "Descent: 200\n",
        "LayerCount: 2\n",
        "Layer: 0 0 \"Back\" 1\n",
        "Layer: 1 0 \"Fore\" 0\n",
        "BeginChars: 3 3\n",
        "StartChar: a+b\n",
        "Encoding: 65536 -1 0\n",
        "Width: 600\n",
        "Fore\n",
        "EndChar\n",
        "StartChar: a+-b\n",
        "Encoding: 65537 -1 1\n",
        "Width: 600\n",
        "Fore\n",
        "EndChar\n",
        "StartChar: a\n",
        "Encoding: 97 97 2\n",
        "Width: 600\n",
        "Fore\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );
    let font = load_str(data).expect("Failed to parse unquoted-name SFD");
    let names: Vec<&str> = font.glyphs.0.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, vec!["a+b", "a+-b", "a"]);
}

#[test]
fn test_load_sfdir() {
    // An SFDir is an exploded SFD: font.props holds the header and each
    // glyph is a standalone StartChar block in its own *.glyph file
    // (including dot-files such as .notdef.glyph). Glyphs must come out
    // in original-GID order, not directory or filename order: in the
    // fixture, b.glyph has GID 1 and a.glyph has GID 2.
    let font =
        load(PathBuf::from("resources/fontforge/simple.sfdir")).expect("Failed to load SFDir");
    let names: Vec<&str> = font.glyphs.0.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, vec![".notdef", "b", "a"]);
    assert_eq!(font.upm, 1000); // Ascent 800 + Descent 200
    assert_eq!(
        font.glyphs.get("a").and_then(|g| g.codepoints.first()),
        Some(&0x61)
    );
    assert_eq!(
        font.glyphs.get("b").and_then(|g| g.codepoints.first()),
        Some(&0x62)
    );
    // Outlines from the glyph files are parsed, not just names
    let b = font.glyphs.get("b").expect("missing glyph b");
    assert!(b.layers.iter().any(|l| l.paths().next().is_some()));
}

#[test]
fn test_quadratic_layer_parses_and_emits_qcurves() {
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "LayerCount: 2\n",
        "Layer: 0 1 \"Back\" 1\n",
        "Layer: 1 1 \"Fore\" 0\n",
        "BeginChars: 1 1\n",
        "StartChar: quad\n",
        "Encoding: -1 -1 0\n",
        "Width: 500\n",
        "Fore\n",
        "SplineSet\n",
        "268 610 m 4,0,1\n",
        " 336 610 336 610 386.5 585.5 c 0x400,-1,2\n",
        "EndSplineSet\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse quadratic SFD");
    let layer = emit::glyph_foreground_layer(&font.glyphs[0], "default").expect("Missing layer");
    let path = layer.paths().next().expect("Missing path");

    assert_eq!(
        layer
            .format_specific
            .get(LAYER_QUADRATIC_KEY)
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(path.nodes.len(), 3);
    assert_eq!(path.nodes[0].nodetype, NodeType::Move);
    assert_eq!(path.nodes[1].nodetype, NodeType::OffCurve);
    assert_eq!(path.nodes[2].nodetype, NodeType::QCurve);

    let emitted = to_str(&font).expect("Failed to emit quadratic SFD");
    assert!(emitted.contains("Layer: 1 1 \"Fore\" 0"));
    assert!(emitted.contains(" 336 610 336 610 386.5 585.5 c 0x400,-1,2"));
}

#[test]
fn test_altuni_adds_alternate_codepoints() {
    // FontForge records a glyph's extra Unicode mappings in AltUni2 as
    // space-separated `uni.vs.reserved` hex triples. Plain alternates
    // (variation selector 0xffffffff) must become additional codepoints so
    // the cmap matches; a real variation selector is skipped, and repeats
    // are de-duplicated.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "LayerCount: 2\n",
        "Layer: 0 1 \"Back\" 1\n",
        "Layer: 1 1 \"Fore\" 0\n",
        "BeginChars: 2 2\n",
        "StartChar: space\n",
        "Encoding: 32 32 0\n",
        "Width: 250\n",
        "AltUni2: 0000a0.ffffffff.0\n",
        "EndChar\n",
        "StartChar: mu\n",
        "Encoding: 181 181 1\n",
        "Width: 500\n",
        "AltUni2: 0003bc.ffffffff.0 0003bc.ffffffff.0 000041.0000fe00.0\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse SFD with AltUni2");
    // space: primary U+0020 plus the plain alternate U+00A0.
    assert_eq!(font.glyphs[0].codepoints, vec![0x0020, 0x00A0]);
    // mu: primary U+00B5 plus U+03BC (the duplicate is collapsed). The U+0041
    // entry has a real variation selector (0xfe00), so it is NOT a codepoint.
    assert_eq!(font.glyphs[1].codepoints, vec![0x00B5, 0x03BC]);
}

#[test]
fn test_oneline_glyph_rules_are_added_to_lookups() {
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 1 0 0 \"Latin Smallcaps Lookup\" {\"Latin Smallcaps\"} [ ]\n",
        "Lookup: 2 0 0 \"Latin Decomp Lookup\" {\"Latin Decomposition\"} [ ]\n",
        "Lookup: 3 0 0 \"Latin Alt Lookup\" {\"Latin Swash\"} [ ]\n",
        "Lookup: 4 0 0 \"Latin Liga Lookup\" {\"Latin Ligatures\"} [ ]\n",
        "Lookup: 257 0 0 \"Inferiors Lookup\" {\"Inferiors\"} [ ]\n",
        "Lookup: 258 0 0 \"Distances Lookup\" {\"Distances\"} [ ]\n",
        "BeginChars: 1 1\n",
        "StartChar: agrave\n",
        "Encoding: -1 224 0\n",
        "Width: 500\n",
        "Fore\n",
        "Substitution2: \"Latin Smallcaps\" agrave.sc\n",
        "AlternateSubs2: \"Latin Swash\" agrave.alt agrave.swash\n",
        "MultipleSubs2: \"Latin Decomposition\" a grave\n",
        "Ligature2: \"Latin Ligatures\" a grave\n",
        "Position2: \"Inferiors\" dx=0 dy=-900 dh=0 dv=0\n",
        "PairPos2: \"Distances\" B dx=0 dy=0 dh=0 dv=0 dx=-10 dy=0 dh=0 dv=0\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse one-line glyph rule SFD");
    let prefixes = font
        .features
        .prefixes
        .values()
        .map(|p| p.code.clone())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(prefixes.contains("sub agrave by agrave.sc;"));
    assert!(prefixes.contains("sub agrave by a grave;"));
    assert!(prefixes.contains("sub agrave from [agrave.alt agrave.swash];"));
    assert!(prefixes.contains("sub a grave by agrave;"));
    assert!(prefixes.contains("pos agrave <0 -900 0 0>;"));
    assert!(prefixes.contains("pos agrave <0 0 0 0> B <-10 0 0 0>;"));
}

#[test]
fn test_indic_mark_anchors_and_subcategory() {
    // AnchorPoint lines appear before the "Fore" marker in SFD and use
    // FontForge anchor-class names. The convertor must (a) keep the anchors
    // by attaching them to the foreground layer, (b) translate the class
    // names into the Glyphs top/bottom (base) and _top/_bottom (mark)
    // convention using the AnchorClass2 above/below classification, and
    // (c) mark GlyphClass-4 glyphs as Nonspacing marks.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        // Two mark-to-base (kind 0x104 = 260) GPOS lookups, one per feature.
        // These are anchor-based and must be skipped on export.
        "Lookup: 260 0 0 \"abvm mark\" {\"abvm-1\"} [ 'abvm' ('deva' <'dflt' > ) ]\n",
        "Lookup: 260 0 0 \"blwm mark\" {\"blwm-1\"} [ 'blwm' ('deva' <'dflt' > ) ]\n",
        "AnchorClass2: \"Above\" \"'abvm' Above Base Mark lookup 1 subtable\" ",
        "\"Below\" \"'blwm' Below Base Mark lookup 2 subtable\"\n",
        "BeginChars: 3 3\n",
        "StartChar: ka\n",
        "Encoding: 0 -1 0\n",
        "Width: 600\n",
        "GlyphClass: 2\n",
        "AnchorPoint: \"Above\" 300 700 basechar 0\n",
        "AnchorPoint: \"Below\" 300 -50 basechar 0\n",
        "Fore\n",
        "EndChar\n",
        "StartChar: anusvara\n",
        "Encoding: 1 -1 1\n",
        "Width: 0\n",
        "GlyphClass: 4\n",
        "AnchorPoint: \"Above\" 0 500 mark 0\n",
        "Fore\n",
        "EndChar\n",
        "StartChar: k_ka\n",
        "Encoding: 2 -1 2\n",
        "Width: 1000\n",
        "GlyphClass: 3\n",
        "Fore\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );
    let font = load_str(data).expect("Failed to parse Indic-mark SFD");
    let default_master_id = font.masters[0].id.clone();

    // Base glyph: still a Base, with its anchors kept under the names the
    // SFD gives them, and attached to the (implicit) foreground layer even
    // though they appear before the "Fore" marker.
    let ka = font.glyphs.get("ka").expect("missing base glyph 'ka'");
    assert_eq!(ka.category, GlyphCategory::Base);
    let ka_layer =
        emit::glyph_foreground_layer(ka, &default_master_id).expect("ka foreground layer");
    let mut ka_names: Vec<&str> = ka_layer.anchors.iter().map(|a| a.name.as_str()).collect();
    ka_names.sort();
    assert_eq!(ka_names, vec!["Above", "Below"], "base anchor names");

    // Mark glyph: category Mark + subCategory Nonspacing.
    let mark = font
        .glyphs
        .get("anusvara")
        .expect("missing mark glyph 'anusvara'");
    assert_eq!(mark.category, GlyphCategory::Mark);
    assert_eq!(
        mark.format_specific
            .get("subcategory")
            .and_then(|v| v.as_str()),
        Some("Nonspacing"),
        "mark glyph must carry Nonspacing subCategory"
    );
    let mark_layer =
        emit::glyph_foreground_layer(mark, &default_master_id).expect("mark foreground layer");
    let mark_names: Vec<&str> = mark_layer.anchors.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(mark_names, vec!["_Above"], "mark anchor name");

    // Ligature glyph (GlyphClass 3): category Ligature + subCategory
    // Ligature, so the exported category becomes a valid Glyphs "Letter".
    let lig = font
        .glyphs
        .get("k_ka")
        .expect("missing ligature glyph 'k_ka'");
    assert_eq!(lig.category, GlyphCategory::Ligature);
    assert_eq!(
        lig.format_specific
            .get("subcategory")
            .and_then(|v| v.as_str()),
        Some("Ligature"),
        "ligature glyph must carry Ligature subCategory"
    );

    // Anchor-based mark GPOS lookups carry no FEA rules, so they must not
    // be emitted as empty feature blocks.
    assert!(
        !font
            .features
            .features
            .iter()
            .any(|(tag, _)| tag == "abvm" || tag == "blwm"),
        "empty anchor-based mark features must not be exported"
    );
}

#[test]
fn test_feature_code_order_is_deterministic() {
    // Rust seeds its hasher per process, so anything built by iterating a
    // HashMap comes out in a different order on every run. The lookup and
    // feature order used to be, which made one unchanged .sfd convert to
    // different .glyphs files and compile to different binaries: 10 of 101
    // families in a Google Fonts corpus, and a QA check on lookup order
    // that passed or failed depending on the run.
    //
    // Iterating within one process cannot vary, so this compares the
    // ordering against the source's own, which is what it must follow.
    let sfd_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/fontforge/Glegoo-Regular.sfd");
    let data = String::from_utf8_lossy(&fs::read(&sfd_path).expect("Missing SFD")).into_owned();
    let font = load_str(&data).expect("Failed to parse Glegoo SFD");

    // Each emitted prefix carries the index of the lookup it came from, so
    // the emitted sequence of indices shows the order that was used.
    let indices: Vec<u32> = font
        .features
        .prefixes
        .keys()
        .filter_map(|name| name.rsplit_once("lookup_"))
        .filter_map(|(_, index)| index.parse().ok())
        .collect();
    assert!(
        indices.len() > 20,
        "test needs a file with many lookups, found {}",
        indices.len()
    );

    // Definition order is declaration order with one deviation: a lookup a
    // chain rule calls is hoisted to just before its caller, since the
    // feature file must define it first. Glegoo declares each contextual
    // lookup ahead of its callees, so every swapped pair below is such a
    // hoist; the final 0 is the first GPOS lookup following the GSUB ones.
    //
    // The exact sequence is asserted because it is what a hash-seeded
    // shuffle destroys and what an ordering-policy change must own up to.
    assert_eq!(
        indices,
        vec![
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14, 15, 13, 17, 16, 19, 18, 21, 20, 23, 22,
            24, 25, 26, 27, 29, 28, 30, 31, 32, 34, 33, 0
        ],
        "lookup definition order changed"
    );
}

#[test]
fn test_chain_pos_sub_parsing() {
    // Test with the Glegoo font, which has ChainSub2 lookups
    let sfd_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/fontforge/Glegoo-Regular.sfd");
    let data = String::from_utf8_lossy(&fs::read(&sfd_path).expect("Missing SFD")).into_owned();
    let font = load_str(&data).expect("Failed to parse Glegoo SFD");

    let fea = font.features.to_fea();

    assert!(
        fea.contains("isign_ra_virama.alt2"),
        "Should contain the coverage match glyphs"
    );
    assert!(
        fea.contains("Single_Substitution_lookup_34"),
        "Should reference the chained lookup"
    );
    assert!(
        fea.contains("rradeva"),
        "Should contain the String match glyphs"
    );
    assert!(
        fea.contains("zerowidthjoiner"),
        "Should contain the second String match glyph"
    );
    // Verify the 'glyph' kind emits individual glyphs with ' markers
    assert!(
        fea.contains("rradeva' lookup Ligature_Substitution_lookup_31 viramadeva'"),
        "Should emit each glyph position with its own lookup"
    );

    // Verify emission ordering: referenced lookups must come before
    // the chain/context lookup that references them.
    let prefixes: Vec<&str> = font
        .features
        .prefixes
        .values()
        .map(|p| p.code.as_str())
        .collect();
    let glegoo_fea = prefixes.join("\n");

    // Find positions of key lookups in the emitted output
    let dep_pos = glegoo_fea
        .find("lookup Ligature_Substitution_lookup_31")
        .expect("Referenced lookup Ligature_Substitution_lookup_31 should exist");
    let chain_pos = glegoo_fea
        .find("lookup _psts__Post_Base_Substitutions_lookup_32")
        .expect("Chain lookup _psts__Post_Base_Substitutions_lookup_32 should exist");
    assert!(
        dep_pos < chain_pos,
        "Referenced lookup must be emitted before the chain lookup that references it"
    );

    let dep2_pos = glegoo_fea
        .find("lookup Single_Substitution_lookup_34")
        .expect("Referenced lookup Single_Substitution_lookup_34 should exist");
    let chain2_pos = glegoo_fea
        .find("lookup _psts__Post_Base_Substitutions_lookup_33")
        .expect("Chain lookup _psts__Post_Base_Substitutions_lookup_33 should exist");
    assert!(
        dep2_pos < chain2_pos,
        "Referenced lookup Single_Substitution_lookup_34 must be emitted before \
             the chain lookup _psts__Post_Base_Substitutions_lookup_33"
    );
}

#[test]
fn test_chain_context_emission() {
    // Minimal inline test
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 6 0 0 \"Chain Lookup\" {\"chain-sub-1\"} [\n",
        "ChainSub2: coverage \"chain-sub-1\"  0 0 0 1\n",
        " 1 0 1\n",
        "  Coverage: 2 glyph_a glyph_b\n",
        "  FCoverage: 1 glyph_c\n",
        " 1\n",
        "  SeqLookup: 0 \"Other Lookup\"\n",
        "EndFPST\n",
        "Lookup: 1 0 0 \"Other Lookup\" {\"other-sub\"} [ ]\n",
        "BeginChars: 4 4\n",
        "StartChar: space\n",
        "Encoding: 32 32 0\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 1\n",
        "Width: 250\n",
        "Substitution2: \"other-sub\" glyph_b\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 2\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: glyph_c\n",
        "Encoding: 99 99 3\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse chain context SFD");
    let fea = font.features.to_fea();

    assert!(
        fea.contains("sub [glyph_a glyph_b]' lookup Other_Lookup glyph_c;"),
        "The chain rule should carry its coverage, lookup and lookahead:\n{fea}"
    );
}

/// The SFD of `test_chain_context_emission` with `Other Lookup` registered only
/// to `aalt`, and with or without the chain rule's call to it.
fn chain_target_sfd(calls_other_lookup: bool) -> String {
    let seq_lookups = if calls_other_lookup {
        " 1\n  SeqLookup: 0 \"Other Lookup\"\n"
    } else {
        " 0\n"
    };
    format!(
        concat!(
            "SplineFontDB: 3.0\n",
            "Lookup: 6 0 0 \"Chain Lookup\" {{\"chain-sub-1\"}} [\n",
            "ChainSub2: coverage \"chain-sub-1\"  0 0 0 1\n",
            " 1 0 1\n",
            "  Coverage: 2 glyph_a glyph_b\n",
            "  FCoverage: 1 glyph_c\n",
            "{seq_lookups}",
            "EndFPST\n",
            "Lookup: 1 0 0 \"Other Lookup\" {{\"other-sub\"}} ['aalt' ('DFLT' <'dflt' > ) ]\n",
            "BeginChars: 4 4\n",
            "StartChar: space\n",
            "Encoding: 32 32 0\n",
            "Width: 250\n",
            "EndChar\n",
            "StartChar: glyph_a\n",
            "Encoding: 97 97 1\n",
            "Width: 250\n",
            "Substitution2: \"other-sub\" glyph_b\n",
            "EndChar\n",
            "StartChar: glyph_b\n",
            "Encoding: 98 98 2\n",
            "Width: 250\n",
            "EndChar\n",
            "StartChar: glyph_c\n",
            "Encoding: 99 99 3\n",
            "Width: 250\n",
            "EndChar\n",
            "EndChars\n",
            "EndSplineFont\n"
        ),
        seq_lookups = seq_lookups
    )
}

#[test]
fn test_aalt_only_lookup_called_from_a_chain_is_still_defined() {
    // A lookup whose only feature registration is `aalt` has its single and
    // alternate substitutions inlined into the feature, so it is not written out
    // as a feature prefix. But a chain context may still call it by name, and that
    // call needs a definition to refer to.
    let data = chain_target_sfd(true);
    let font = load_str(&data).expect("Failed to parse chain context SFD");
    let fea = font.features.to_fea();

    assert!(
        fea.contains("sub [glyph_a glyph_b]' lookup Other_Lookup glyph_c;"),
        "The chain rule should still call the aalt-only lookup:\n{fea}"
    );
    assert!(
        fea.contains("lookup Other_Lookup {"),
        "...and the lookup it calls must be defined, or the call dangles:\n{fea}"
    );
}

#[test]
fn test_aalt_only_lookup_nothing_calls_is_not_defined() {
    // The converse, and the reason the suppression exists: with no chain calling
    // it, an aalt-only lookup written out as a prefix is a named lookup nothing
    // references.
    let data = chain_target_sfd(false);
    let font = load_str(&data).expect("Failed to parse chain context SFD");
    let fea = font.features.to_fea();

    assert!(
        !fea.contains("lookup Other_Lookup {"),
        "An aalt-only lookup no chain calls should not be emitted:\n{fea}"
    );
    assert!(
        fea.contains("feature aalt {\nsub glyph_a by glyph_b;"),
        "aalt must still carry the inlined rule:\n{fea}"
    );
}

#[test]
fn test_class_fpst_emission() {
    // A `class` FPST defines its glyph classes once and then lists rules that
    // reference them by index. Class 0 is never written: it means "any glyph not
    // in one of the other classes", and has to be expanded from the glyph list.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 6 0 0 \"Chain Lookup\" {\"chain-sub-1\"} [\n",
        "ChainSub2: class \"chain-sub-1\" 3 1 3 2\n",
        "  Class: 7 glyph_a\n",
        "  Class: 7 glyph_b\n",
        "  FClass: 7 glyph_c\n",
        "  FClass: 7 glyph_d\n",
        " 2 0 0\n",
        "  ClsList: 1 2\n",
        "  BClsList:\n",
        "  FClsList:\n",
        " 1\n",
        "  SeqLookup: 0 \"Other Lookup\"\n",
        " 1 0 2\n",
        "  ClsList: 1\n",
        "  BClsList:\n",
        "  FClsList: 0 2\n",
        " 1\n",
        "  SeqLookup: 0 \"Other Lookup\"\n",
        "  ClassNames: \"All_Others\" \"a\" \"b\"\n",
        "EndFPST\n",
        "Lookup: 1 0 0 \"Other Lookup\" {\"other-sub\"} [ ]\n",
        "BeginChars: 5 5\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "Substitution2: \"other-sub\" glyph_b\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 1\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: glyph_c\n",
        "Encoding: 99 99 2\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: glyph_d\n",
        "Encoding: 100 100 3\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: space\n",
        "Encoding: 32 32 4\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse class FPST SFD");
    let fea = font.features.to_fea();

    // Both rules of the subtable must be emitted, not just the first.
    assert_eq!(
        fea.matches("' lookup Other_Lookup").count(),
        2,
        "Each rule of a class FPST should produce a line:\n{fea}"
    );
    // Rule 1: two input classes, the lookup applied at position 0.
    assert!(
        fea.contains("sub glyph_a' lookup Other_Lookup glyph_b';"),
        "Class indices should resolve to their glyph lists:\n{fea}"
    );
    // Rule 2: `FClsList: 0 2` is "any other glyph, then class 2 (glyph_d)".
    // Class 0 must expand to the glyphs not named by FClass 1 or FClass 2.
    // A multi-glyph class is declared once and referred to, the way FontForge's
    // own export writes it, rather than repeated at every position.
    assert!(
        fea.contains("@Chain_Lookup_c1 = [glyph_a glyph_b space];"),
        "Class 0 should expand to every glyph not in a sibling class:\n{fea}"
    );
    assert!(
        fea.contains("sub glyph_a' lookup Other_Lookup @Chain_Lookup_c1 glyph_d;"),
        "The rule should refer to the declared class:\n{fea}"
    );
}

#[test]
fn test_class_fpst_backtrack_order_is_preserved() {
    // FontForge stores backtrack classes in feature-file order, farthest
    // from the input first, and its own SFD -> FEA export writes them that
    // way: Monomakh's `BClsList: 3 1` becomes
    // `sub @cc22_back_3 @cc22_back_1 @cc22_match_4' ...`. Reversing here
    // inverts every rule with more than one backtrack class.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 6 0 0 \"Chain Lookup\" {\"chain-sub-1\"} [\n",
        "ChainSub2: class \"chain-sub-1\" 2 3 1 1\n",
        "  Class: 7 glyph_c\n",
        "  BClass: 7 glyph_a\n",
        "  BClass: 7 glyph_b\n",
        " 1 2 0\n",
        "  ClsList: 1\n",
        "  BClsList: 2 1\n",
        "  FClsList:\n",
        " 1\n",
        "  SeqLookup: 0 \"Other Lookup\"\n",
        "EndFPST\n",
        "Lookup: 1 0 0 \"Other Lookup\" {\"other-sub\"} [ ]\n",
        "BeginChars: 3 3\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "Substitution2: \"other-sub\" glyph_b\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 1\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: glyph_c\n",
        "Encoding: 99 99 2\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse class FPST SFD");
    let fea = font.features.to_fea();

    // BClass 1 is glyph_a and BClass 2 is glyph_b, so `BClsList: 2 1`
    // must emit glyph_b then glyph_a, in that order.
    assert!(
        fea.contains("sub glyph_b glyph_a glyph_c' lookup Other_Lookup;"),
        "Backtrack classes must keep their on-disk order:\n{fea}"
    );
}

#[test]
fn test_class_fpst_ponomar() {
    // Ponomar's contextual lookups are all `class`-kind, so the contextual half
    // of its `ccmp` rests entirely on this path.
    let sfd_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/fontforge/Ponomar.sfd");
    let data = String::from_utf8_lossy(&fs::read(&sfd_path).expect("Missing SFD")).into_owned();
    let font = load_str(&data).expect("Failed to parse Ponomar SFD");
    let fea = font.features.to_fea();

    assert!(
        fea.contains("' lookup Psi_Truncation"),
        "Class-kind chain rules should reference their nested lookup"
    );
    assert!(
        fea.contains("feature ccmp"),
        "ccmp is built only from class-kind contextual lookups"
    );
}

#[test]
fn test_feature_references_follow_declaration_order() {
    // Definitions are hoisted so a lookup exists before a chain references it,
    // while the references inside the feature block keep declaration order,
    // exactly as FontForge's own export writes them. (The compiled font follows
    // the definition order; the reference order is for the reader.)
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 6 0 0 \"Aaa First\" {\"chain-sub-1\"} ['calt' ('DFLT' <'dflt'>)]\n",
        "ChainSub2: coverage \"chain-sub-1\"  0 0 0 1\n",
        " 1 0 0\n",
        "  Coverage: 7 glyph_a\n",
        " 1\n",
        "  SeqLookup: 0 \"Bbb Second\"\n",
        "EndFPST\n",
        "Lookup: 1 0 0 \"Bbb Second\" {\"second-sub\"} ['calt' ('DFLT' <'dflt'>)]\n",
        "BeginChars: 2 2\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "Substitution2: \"second-sub\" glyph_b\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 1\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse SFD");
    let fea = font.features.to_fea();

    let block_start = fea.find("feature calt {").expect("calt feature emitted");
    let block = &fea[block_start..];
    let first = block
        .find("lookup Aaa_First;")
        .expect("calt should reference the chain lookup");
    let second = block
        .find("lookup Bbb_Second;")
        .expect("calt should reference the simple lookup");
    assert!(
        first < second,
        "References must follow declaration order, not the definition buckets:\n{fea}"
    );
    let def = fea
        .find("lookup Bbb_Second {")
        .expect("the simple lookup should be defined");
    let chain_def = fea.find("lookup Aaa_First {").expect("chain defined");
    assert!(
        def < chain_def,
        "The referenced lookup must still be defined before the chain that calls \
             it:\n{fea}"
    );
}

#[test]
fn test_call_to_an_empty_lookup_becomes_ignore() {
    // A lookup with no rules is never written out, so a call to it cannot be
    // emitted as a reference. But the rule still matches, and a match with
    // nothing to do stops later rules at that position -- which is exactly what
    // `ignore` says. Dropping the rule instead would let those later rules fire.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 6 0 0 \"Chain Lookup\" {\"chain-sub-1\"} ['calt' ('DFLT' <'dflt'>)]\n",
        "ChainSub2: coverage \"chain-sub-1\"  0 0 0 1\n",
        " 1 0 1\n",
        "  Coverage: 7 glyph_a\n",
        "  FCoverage: 7 glyph_b\n",
        " 1\n",
        "  SeqLookup: 0 \"Empty Lookup\"\n",
        "EndFPST\n",
        "Lookup: 1 0 0 \"Empty Lookup\" {\"empty-sub\"} [ ]\n",
        "BeginChars: 2 2\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 1\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse coverage FPST SFD");
    let fea = font.features.to_fea();

    assert!(
        fea.contains("ignore sub glyph_a' glyph_b;"),
        "A call to an empty lookup should leave an ignore rule, not a dangling \
             reference and not a dropped rule:\n{fea}"
    );
    assert!(
        !fea.contains("lookup Empty_Lookup"),
        "The empty lookup must not be referenced or defined:\n{fea}"
    );
}

#[test]
fn test_seqlookup_index_past_the_last_position_drops_the_rule() {
    // A rule that names lookups but lands none of them is neither the substitution
    // the section asked for nor an `ignore`, and fea-rs cannot represent it.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 6 0 0 \"Chain Lookup\" {\"chain-sub-1\"} ['calt' ('DFLT' <'dflt'>)]\n",
        "ChainSub2: glyph \"chain-sub-1\"  0 0 0 1\n",
        " String: 7 glyph_a\n",
        " BString: 0 \n",
        " FString: 0 \n",
        " 1\n",
        "  SeqLookup: 5 \"Other Lookup\"\n",
        "EndFPST\n",
        "Lookup: 1 0 0 \"Other Lookup\" {\"other-sub\"} [ ]\n",
        "BeginChars: 1 1\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse glyph FPST SFD");
    let fea = font.features.to_fea();

    assert!(
        !fea.contains("glyph_a'"),
        "A rule whose lookups all miss must not be emitted:\n{fea}"
    );
}

#[test]
fn test_reference_resolves_to_the_lookup_that_was_defined() {
    // Two SFD lookup names can sanitize alike; the second is defined as `_2`. A
    // reference that just re-sanitizes would resolve to the first one.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 1 0 0 \"My Lookup\" {\"first-sub\"} [ ]\n",
        "Lookup: 1 0 0 \"My-Lookup\" {\"second-sub\"} [ ]\n",
        "Lookup: 6 0 0 \"Chain Lookup\" {\"chain-sub-1\"} ['calt' ('DFLT' <'dflt'>)]\n",
        "ChainSub2: glyph \"chain-sub-1\"  0 0 0 1\n",
        " String: 7 glyph_a\n",
        " BString: 0 \n",
        " FString: 0 \n",
        " 1\n",
        "  SeqLookup: 0 \"My-Lookup\"\n",
        "EndFPST\n",
        "BeginChars: 1 1\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "Substitution2: \"first-sub\" glyph_a\n",
        "Substitution2: \"second-sub\" glyph_a\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse glyph FPST SFD");
    let fea = font.features.to_fea();

    assert!(
        fea.contains("sub glyph_a' lookup My_Lookup_2;"),
        "The rule names the second lookup, so it must resolve to My_Lookup_2:\n{fea}"
    );
}

#[test]
fn test_generated_suffix_names_cannot_be_taken_twice() {
    // "My Lookup" and "My-Lookup" sanitize alike, so the second is assigned
    // My_Lookup_2. A third lookup literally NAMED "My Lookup 2" also sanitizes
    // to My_Lookup_2; if generated names were not registered as taken, it would
    // silently replace the second lookup wholesale.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 1 0 0 \"My Lookup\" {\"first-sub\"} ['calt' ('DFLT' <'dflt'>)]\n",
        "Lookup: 1 0 0 \"My-Lookup\" {\"second-sub\"} ['calt' ('DFLT' <'dflt'>)]\n",
        "Lookup: 1 0 0 \"My Lookup 2\" {\"third-sub\"} ['calt' ('DFLT' <'dflt'>)]\n",
        "BeginChars: 2 2\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "Substitution2: \"first-sub\" glyph_b\n",
        "Substitution2: \"second-sub\" glyph_b\n",
        "Substitution2: \"third-sub\" glyph_b\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 1\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse SFD");
    let fea = font.features.to_fea();

    for name in ["My_Lookup {", "My_Lookup_2 {", "My_Lookup_2_2 {"] {
        assert!(
            fea.contains(name),
            "All three lookups must survive with distinct names, missing {name:?}:\n{fea}"
        );
    }
}

#[test]
fn test_keyword_and_digit_lookup_names_get_safe_labels() {
    // A lookup literally named "sub" or "2 Alternates" produced a label that
    // fails to parse ("Expected LABEL found SubKw" / "Expected ID found NUM"),
    // aborting the whole conversion rather than one lookup.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 1 0 0 \"sub\" {\"first-sub\"} ['calt' ('DFLT' <'dflt'>)]\n",
        "Lookup: 1 0 0 \"2 Alternates\" {\"second-sub\"} ['calt' ('DFLT' <'dflt'>)]\n",
        "BeginChars: 2 2\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "Substitution2: \"first-sub\" glyph_b\n",
        "Substitution2: \"second-sub\" glyph_b\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 1\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse SFD");
    let fea = font.features.to_fea();

    assert!(
        fea.contains("lookup sub_ {"),
        "A keyword name takes a trailing underscore:\n{fea}"
    );
    assert!(
        fea.contains("lookup _2_Alternates {"),
        "A digit-leading name takes a leading underscore:\n{fea}"
    );
}

#[test]
fn test_utf7_escaped_lookup_and_subtable_names_still_match() {
    // FontForge escapes names as UTF-7, where `+-` is a literal `+`. Decoding the
    // reference but not the definition, or the section key but not the subtable
    // name, loses the rule.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 6 0 0 \"calt+-probe\" {\"calt+-sub-1\"} ['calt' ('DFLT' <'dflt'>)]\n",
        "ChainSub2: glyph \"calt+-sub-1\"  0 0 0 1\n",
        " String: 7 glyph_a\n",
        " BString: 0 \n",
        " FString: 0 \n",
        " 1\n",
        "  SeqLookup: 0 \"Other+-Lookup\"\n",
        "EndFPST\n",
        "Lookup: 1 0 0 \"Other+-Lookup\" {\"other-sub\"} [ ]\n",
        "BeginChars: 1 1\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "Substitution2: \"other-sub\" glyph_a\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse glyph FPST SFD");
    let fea = font.features.to_fea();

    assert!(
        fea.contains("sub glyph_a' lookup Other_Lookup;"),
        "A UTF-7 escaped name must match on both sides:\n{fea}"
    );
}

#[test]
fn test_utf7_escaped_subtable_name_still_finds_its_oneline_rules() {
    // A Ligature2/Substitution2 line names its subtable too. Decoding the name on
    // the Lookup: line but not here leaves the rule unable to find its subtable,
    // and the whole lookup is lost.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 4 0 0 \"liga+-lookup\" {\"liga+-sub-1\"} ['liga' ('DFLT' <'dflt'>)]\n",
        "BeginChars: 3 3\n",
        "StartChar: f\n",
        "Encoding: 102 102 0\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: i\n",
        "Encoding: 105 105 1\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: f_i\n",
        "Encoding: -1 -1 2\n",
        "Width: 250\n",
        "Ligature2: \"liga+-sub-1\" f i\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse SFD");
    let fea = font.features.to_fea();

    assert!(
        fea.contains("sub f i by f_i;"),
        "A one-line rule must find its subtable through the decoded name:\n{fea}"
    );
}

#[test]
fn test_fpst_drops_glyphs_the_font_does_not_have() {
    // An SFD keeps the class lists of glyphs that were later deleted. Writing those
    // names into the feature file gives the compiler a glyph it cannot resolve, so
    // a class position drops the absent names and keeps the rest.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 6 0 0 \"Chain Lookup\" {\"chain-sub-1\"} ['calt' ('DFLT' <'dflt'>)]\n",
        "ChainSub2: class \"chain-sub-1\" 2 1 1 1\n",
        "  Class: 23 glyph_a glyph_gone\n",
        " 1 0 0\n",
        "  ClsList: 1\n",
        "  BClsList:\n",
        "  FClsList:\n",
        " 1\n",
        "  SeqLookup: 0 \"Other Lookup\"\n",
        "EndFPST\n",
        "Lookup: 1 0 0 \"Other Lookup\" {\"other-sub\"} [ ]\n",
        "BeginChars: 2 2\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "Substitution2: \"other-sub\" glyph_b\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 1\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse class FPST SFD");
    let fea = font.features.to_fea();

    assert!(
        !fea.contains("glyph_gone"),
        "A glyph the font does not have must not reach the feature file:\n{fea}"
    );
    assert!(
        fea.contains("sub glyph_a' lookup Other_Lookup;"),
        "The rest of the class should survive:\n{fea}"
    );
}

#[test]
fn test_glyph_kind_rule_with_a_missing_glyph_is_dropped() {
    // In a glyph-kind rule each glyph is a position that SeqLookup indices count,
    // so an absent one cannot simply be removed: that would move the lookup onto
    // its neighbour. The rule goes instead.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 6 0 0 \"Chain Lookup\" {\"chain-sub-1\"} ['calt' ('DFLT' <'dflt'>)]\n",
        "ChainSub2: glyph \"chain-sub-1\"  0 0 0 1\n",
        " String: 23 glyph_gone glyph_a\n",
        " BString: 0 \n",
        " FString: 0 \n",
        " 1\n",
        "  SeqLookup: 0 \"Other Lookup\"\n",
        "EndFPST\n",
        "Lookup: 1 0 0 \"Other Lookup\" {\"other-sub\"} [ ]\n",
        "BeginChars: 2 2\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 1\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse glyph FPST SFD");
    let fea = font.features.to_fea();

    assert!(
        !fea.contains("glyph_gone"),
        "A glyph the font does not have must not reach the feature file:\n{fea}"
    );
    assert!(
        !fea.contains("glyph_a'"),
        "Pruning the position would move the lookup onto glyph_a, so the rule \
             must be dropped whole:\n{fea}"
    );
}

#[test]
fn test_coverage_kind_backtrack_is_turned_round() {
    // A coverage section stores BCoverage the way OpenType does, nearest the input
    // first, unlike a class section whose BClsList is already in feature-file
    // order. Donegal One's ordinals are the real case: BCoverage period, then the
    // digits, and FontForge's own build puts period nearest the input, so the text
    // reads "digit period o".
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 6 0 0 \"Chain Lookup\" {\"chain-sub-1\"} [\n",
        "ChainSub2: coverage \"chain-sub-1\"  0 0 0 1\n",
        " 1 2 0\n",
        "  Coverage: 7 glyph_c\n",
        "  BCoverage: 7 glyph_a\n",
        "  BCoverage: 7 glyph_b\n",
        " 1\n",
        "  SeqLookup: 0 \"Other Lookup\"\n",
        "EndFPST\n",
        "Lookup: 1 0 0 \"Other Lookup\" {\"other-sub\"} [ ]\n",
        "BeginChars: 3 3\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "Substitution2: \"other-sub\" glyph_b\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 1\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: glyph_c\n",
        "Encoding: 99 99 2\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse coverage FPST SFD");
    let fea = font.features.to_fea();

    assert!(
        fea.contains("sub glyph_b glyph_a glyph_c' lookup Other_Lookup;"),
        "BCoverage is nearest-first and must be reversed for the feature file:\n{fea}"
    );
}

#[test]
fn test_coverage_section_with_two_rules_keeps_them_apart() {
    // A rule opens with its bare `<ninput> <nbacktrack> <nlookahead>` line, so a
    // section holds as many rules as its header declares. Reading the whole body
    // as one rule concatenates the inputs and stacks every lookup on position 0.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 6 0 0 \"Chain Lookup\" {\"chain-sub-1\"} ['calt' ('DFLT' <'dflt'>)]\n",
        "ChainSub2: coverage \"chain-sub-1\"  0 0 0 2\n",
        " 1 0 1\n",
        "  Coverage: 7 glyph_a\n",
        "  FCoverage: 7 glyph_b\n",
        " 1\n",
        "  SeqLookup: 0 \"Lookup One\"\n",
        " 1 0 1\n",
        "  Coverage: 7 glyph_c\n",
        "  FCoverage: 7 glyph_d\n",
        " 1\n",
        "  SeqLookup: 0 \"Lookup Two\"\n",
        "EndFPST\n",
        "Lookup: 1 0 0 \"Lookup One\" {\"one-sub\"} [ ]\n",
        "Lookup: 1 0 0 \"Lookup Two\" {\"two-sub\"} [ ]\n",
        "BeginChars: 4 4\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "Substitution2: \"one-sub\" glyph_b\n",
        "Substitution2: \"two-sub\" glyph_b\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 1\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: glyph_c\n",
        "Encoding: 99 99 2\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: glyph_d\n",
        "Encoding: 100 100 3\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse coverage FPST SFD");
    let fea = font.features.to_fea();

    assert!(
        fea.contains("sub glyph_a' lookup Lookup_One glyph_b;"),
        "The first rule should carry only its own positions and lookup:\n{fea}"
    );
    assert!(
        fea.contains("sub glyph_c' lookup Lookup_Two glyph_d;"),
        "The second rule should be its own statement:\n{fea}"
    );
}

#[test]
fn test_glyph_kind_backtrack_is_one_position_per_glyph() {
    // A glyph-kind BString spells one glyph per backtrack position, the same way
    // its String: spells the input. Treating the line as a single position turns a
    // two-glyph sequence into a class matching either glyph.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 6 0 0 \"Chain Lookup\" {\"chain-sub-1\"} [\n",
        "ChainSub2: glyph \"chain-sub-1\"  0 0 0 1\n",
        " String: 7 glyph_c\n",
        " BString: 15 glyph_a glyph_b\n",
        " FString: 0 \n",
        " 1\n",
        "  SeqLookup: 0 \"Other Lookup\"\n",
        "EndFPST\n",
        "Lookup: 1 0 0 \"Other Lookup\" {\"other-sub\"} [ ]\n",
        "BeginChars: 3 3\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "Substitution2: \"other-sub\" glyph_b\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 1\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: glyph_c\n",
        "Encoding: 99 99 2\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse glyph FPST SFD");
    let fea = font.features.to_fea();

    assert!(
        fea.contains("sub glyph_b glyph_a glyph_c' lookup Other_Lookup;"),
        "Each BString glyph is its own position, nearest the input first:\n{fea}"
    );
    assert!(
        !fea.contains("[glyph_a glyph_b]"),
        "A backtrack sequence must not collapse into one glyph class:\n{fea}"
    );
}

#[test]
fn test_glyph_kind_lookups_follow_their_position() {
    // A glyph-kind section spells its input as one `String:` line, but each glyph
    // is a separate input position and SeqLookup indices count positions. Keying
    // the lookups off the group instead puts position 0's lookups on every glyph.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 6 0 0 \"Chain Lookup\" {\"chain-sub-1\"} [\n",
        "ChainSub2: glyph \"chain-sub-1\"  0 0 0 1\n",
        " String: 15 glyph_a glyph_b\n",
        " BString: 0 \n",
        " FString: 0 \n",
        " 2\n",
        "  SeqLookup: 0 \"Lookup One\"\n",
        "  SeqLookup: 1 \"Lookup Two\"\n",
        "EndFPST\n",
        "Lookup: 1 0 0 \"Lookup One\" {\"one-sub\"} [ ]\n",
        "Lookup: 1 0 0 \"Lookup Two\" {\"two-sub\"} [ ]\n",
        "BeginChars: 2 2\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "Substitution2: \"one-sub\" glyph_b\n",
        "Substitution2: \"two-sub\" glyph_b\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 1\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse glyph FPST SFD");
    let fea = font.features.to_fea();

    assert!(
        fea.contains("sub glyph_a' lookup Lookup_One glyph_b' lookup Lookup_Two;"),
        "Each glyph should carry the lookups of its own position:\n{fea}"
    );
}

#[test]
fn test_class_fpst_lookup_at_a_later_position() {
    // SeqLookup indices count input positions. A lookup attached to position 1
    // must land on the second glyph, not the first: a rule that resolved a
    // position away, or keyed the map wrongly, would silently move it.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 6 0 0 \"Chain Lookup\" {\"chain-sub-1\"} [\n",
        "ChainSub2: class \"chain-sub-1\" 3 1 1 1\n",
        "  Class: 7 glyph_a\n",
        "  Class: 7 glyph_b\n",
        " 2 0 0\n",
        "  ClsList: 1 2\n",
        "  BClsList:\n",
        "  FClsList:\n",
        " 1\n",
        "  SeqLookup: 1 \"Other Lookup\"\n",
        "EndFPST\n",
        "Lookup: 1 0 0 \"Other Lookup\" {\"other-sub\"} [ ]\n",
        "BeginChars: 2 2\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "Substitution2: \"other-sub\" glyph_b\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 1\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse class FPST SFD");
    let fea = font.features.to_fea();

    assert!(
        fea.contains("sub glyph_a' glyph_b' lookup Other_Lookup;"),
        "The lookup belongs on the second position, not the first:\n{fea}"
    );
}

#[test]
fn test_class_fpst_emission_order_is_stable() {
    // The lookup ordering list is seeded from a map's iteration order, so that
    // map must be ordered: converting one unchanged file twice has to emit the
    // same lookups in the same order. Ponomar exercises it because all of its
    // contextual lookups feed that map.
    let sfd_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/fontforge/Ponomar.sfd");
    let data = String::from_utf8_lossy(&fs::read(&sfd_path).expect("Missing SFD")).into_owned();

    let first = load_str(&data)
        .expect("Failed to parse Ponomar SFD")
        .features
        .to_fea();
    let second = load_str(&data)
        .expect("Failed to parse Ponomar SFD")
        .features
        .to_fea();

    assert_eq!(
        first, second,
        "Converting the same SFD twice must produce the same feature file"
    );
}

#[test]
fn test_fpst_header_counts_detect_a_misread_body() {
    use super::layout::{FpstBodyCounts, FpstHeaderCounts};

    // Monomakh's calt header: 5 classes each way, 3 rules, and a body holding
    // four `Class:` lines, because class 0 is implicit.
    let header = FpstHeaderCounts {
        classes: 5,
        backtrack_classes: 5,
        lookahead_classes: 5,
        rules: 3,
    };
    let good = FpstBodyCounts {
        classes: 4,
        backtrack_classes: 4,
        lookahead_classes: 4,
        rules: 3,
    };
    assert!(
        header.mismatches(&good).is_empty(),
        "A consistent section must not warn: {:?}",
        header.mismatches(&good)
    );

    // A section declaring nothing has no classes at all, not "minus one".
    let empty = FpstHeaderCounts {
        classes: 0,
        backtrack_classes: 0,
        lookahead_classes: 0,
        rules: 0,
    };
    let nothing = FpstBodyCounts {
        classes: 0,
        backtrack_classes: 0,
        lookahead_classes: 0,
        rules: 0,
    };
    assert!(empty.mismatches(&nothing).is_empty());

    // A rule the parser failed to see is exactly what this catches.
    let short = FpstBodyCounts { rules: 2, ..good };
    assert_eq!(header.mismatches(&short).len(), 1);
    assert!(header.mismatches(&short)[0].contains("declares 3 rules"));
}

#[test]
fn test_class_fpst_ponomar_contextual_kerning() {
    // Ponomar's two ContextPos2 sections are class-kind contextual kerning. They
    // exercise the positioning half of the parser, which the substitution tests
    // never reach.
    let sfd_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/fontforge/Ponomar.sfd");
    let data = String::from_utf8_lossy(&fs::read(&sfd_path).expect("Missing SFD")).into_owned();
    let font = load_str(&data).expect("Failed to parse Ponomar SFD");
    let fea = font.features.to_fea();

    let rule = fea
        .lines()
        .find(|l| l.contains("lookup Combining_Letter_Kerning") && l.contains('\''))
        .expect("Ponomar's contextual kerning rule should be emitted");
    assert!(
        rule.trim_start().starts_with("pos "),
        "A ContextPos2 rule must be emitted as `pos`, not `sub`:\n{rule}"
    );
}

#[test]
fn test_class_fpst_ignore_rule() {
    // A rule with a SeqLookup count of zero matches in order to stop a later,
    // broader rule from firing. FontForge's own SFD -> FEA export writes those
    // as `ignore sub`; a rule with no action at all means something else.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 6 0 0 \"Chain Lookup\" {\"chain-sub-1\"} [\n",
        "ChainSub2: class \"chain-sub-1\" 2 1 2 1\n",
        "  Class: 7 glyph_a\n",
        "  FClass: 7 glyph_b\n",
        " 1 0 1\n",
        "  ClsList: 1\n",
        "  BClsList:\n",
        "  FClsList: 1\n",
        " 0\n",
        "EndFPST\n",
        "BeginChars: 2 2\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 1\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse class FPST SFD");
    let fea = font.features.to_fea();

    assert!(
        fea.contains("ignore sub glyph_a' glyph_b;"),
        "A rule calling no lookup should be an `ignore` rule:\n{fea}"
    );
}

#[test]
fn test_class_fpst_unsatisfiable_position_drops_the_rule_and_its_reference() {
    // "All_Others" is empty when its sibling classes cover the whole font. No
    // glyph can occupy that position, so the rule can never fire. Leaving the
    // position out instead would silently widen the rule, and writing it as `[]`
    // is not feature-file syntax -- so the rule goes. When that empties the whole
    // lookup, the feature must not go on referencing it either, or the FEA names
    // a lookup that is never defined and fontc rejects the font.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 6 0 0 \"Chain Lookup\" {\"chain-sub-1\"} ['calt' ('DFLT' <'dflt'>)]\n",
        "ChainSub2: class \"chain-sub-1\" 2 1 3 1\n",
        "  Class: 7 glyph_a\n",
        "  FClass: 7 glyph_a\n",
        "  FClass: 7 glyph_b\n",
        " 1 0 1\n",
        "  ClsList: 1\n",
        "  BClsList:\n",
        "  FClsList: 0\n",
        " 1\n",
        "  SeqLookup: 0 \"Other Lookup\"\n",
        "EndFPST\n",
        "Lookup: 1 0 0 \"Other Lookup\" {\"other-sub\"} [ ]\n",
        "BeginChars: 2 2\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 1\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse class FPST SFD");
    let fea = font.features.to_fea();

    assert!(
        !fea.contains("[]"),
        "An empty All_Others must not be written as an empty glyph class:\n{fea}"
    );
    assert!(
        !fea.contains("glyph_a'"),
        "The rule can never match, so it must not be emitted:\n{fea}"
    );
    assert!(
        !fea.contains("lookup Chain_Lookup;"),
        "A lookup left with no rules must not still be referenced by its feature:\n{fea}"
    );
}

#[test]
fn test_reverse_chain_is_not_emitted_as_a_forward_rule() {
    // Reverse chaining substitutes inline from a replacement list; a feature
    // file spells it `rsub ... by ...`. Emitting it as a forward `sub` gives a
    // rule with no action, and an actionless rule is an `ignore` -- which would
    // suppress substitutions in exactly the context that wanted one.
    let data = concat!(
        "SplineFontDB: 3.0\n",
        "Lookup: 0 0 0 \"rev probe\" {\"rev-sub-1\"} [\n",
        "ReverseChain2: coverage \"rev-sub-1\"  0 0 0 1\n",
        " 1 0 1\n",
        "  Coverage: 2 glyph_a glyph_b\n",
        "  FCoverage: 1 glyph_c\n",
        "  Replace: 2 glyph_x glyph_y\n",
        "EndFPST\n",
        "BeginChars: 3 3\n",
        "StartChar: glyph_a\n",
        "Encoding: 97 97 0\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: glyph_b\n",
        "Encoding: 98 98 1\n",
        "Width: 250\n",
        "EndChar\n",
        "StartChar: glyph_c\n",
        "Encoding: 99 99 2\n",
        "Width: 250\n",
        "EndChar\n",
        "EndChars\n",
        "EndSplineFont\n"
    );

    let font = load_str(data).expect("Failed to parse reverse chain SFD");
    let fea = font.features.to_fea();

    assert!(
        !fea.contains("ignore"),
        "A reverse chaining lookup must not become an ignore rule:\n{fea}"
    );
    assert!(
        !fea.contains("glyph_a"),
        "It must not be emitted as a forward contextual rule either:\n{fea}"
    );
}

/// FontForge pads a short vendor to four bytes with NULs and writes them
/// inside the quotes, so an SFD can carry `OS2Vendor: 'STC\0'`. A NUL is not
/// legal in an OpenType tag, and the compiler rejects the entire font with
/// "Invalid tag": 22 of 100 Google Fonts families whose upstream source is
/// an SFD failed to build on this alone.
#[test]
fn nul_padded_vendor_ids_are_accepted() {
    let cases = [
        ("'STC\u{0}'", "STC "),
        ("'LTT\u{0}'", "LTT "),
        ("'TT\u{0}\u{0}'", "TT  "),
        ("'PYRS'", "PYRS"),
    ];
    for (raw, want) in cases {
        let sfd = format!(
                "SplineFontDB: 3.0\nFontName: Test\nOS2Vendor: {raw}\nBeginChars: 1 1\nEndChars\nEndSplineFont\n"
            );
        let dir = std::env::temp_dir().join("babelfont_vendor_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.sfd");
        std::fs::write(&path, &sfd).unwrap();
        let font = fontforge::load(path.clone()).expect("SFD should load");
        let tag = font
            .custom_ot_values
            .os2_vendor_id
            .unwrap_or_else(|| panic!("no vendor id for {raw}"));
        assert_eq!(tag.to_string(), want, "vendor {raw}");
        let _ = std::fs::remove_file(&path);
    }
}

use crate::convertors::fontforge;

/// `OS2_UseTypoMetrics` and `OS2_WeightWidthSlopeOnly` are fsSelection bits
/// 7 and 8. They used to be OR'd into `os2_fs_type`, which meant a source
/// declaring `FSType: 0` came out announcing fsType 128 -- bit 7 of fsType
/// is reserved, so the value was not merely wrong but meaningless.
///
/// Measured when this was found: of 100 Google Fonts families whose upstream
/// source is an SFD, 81 declare `OS2_UseTypoMetrics`, and every one of them
/// ships fsType 0.
#[test]
fn use_typo_metrics_goes_to_fsselection_not_fstype() {
    let sfd = "\
SplineFontDB: 3.0
FontName: Test
FSType: 0
OS2Version: 2
OS2_UseTypoMetrics: 1
OS2_WeightWidthSlopeOnly: 1
BeginChars: 1 1
EndChars
EndSplineFont
";
    let dir = std::env::temp_dir().join("babelfont_fsselection_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.sfd");
    std::fs::write(&path, sfd).unwrap();

    let font = fontforge::load(path.clone()).expect("SFD should load");
    let ot = &font.custom_ot_values;

    assert_eq!(
        ot.os2_fs_type,
        Some(0),
        "fsType must stay what the SFD said"
    );
    let fs_selection = ot.os2_fs_selection.expect("fsSelection should be set");
    assert!(fs_selection & (1 << 7) != 0, "USE_TYPO_METRICS is bit 7");
    assert!(fs_selection & (1 << 8) != 0, "WWS is bit 8");

    let _ = std::fs::remove_file(&path);
}

use super::is_single_or_alternate_sub;

#[test]
fn only_single_and_alternate_subs_may_go_in_aalt() {
    // These are what aalt is allowed to contain.
    assert!(is_single_or_alternate_sub("sub i by i.alt;"));
    assert!(is_single_or_alternate_sub("    sub a by a.sc;"));
    assert!(is_single_or_alternate_sub("sub a from [a.alt1 a.alt2];"));
    assert!(is_single_or_alternate_sub("substitute i by i.alt;"));

    // These are not: a ligature, a multiple, a class-to-class rule, and a
    // contextual rule. Letting any of them through would produce feature
    // code the compiler rejects outright.
    assert!(!is_single_or_alternate_sub("sub f i by f_i;"));
    assert!(!is_single_or_alternate_sub("sub f_i by f i;"));
    assert!(!is_single_or_alternate_sub("sub [a b] by [a.alt b.alt];"));
    assert!(!is_single_or_alternate_sub("sub a' lookup foo b;"));
    assert!(!is_single_or_alternate_sub("sub @CLASS by @OTHER;"));
    assert!(!is_single_or_alternate_sub("lookup _aalt_lookup_0;"));
    assert!(!is_single_or_alternate_sub("script DFLT;"));
    assert!(!is_single_or_alternate_sub("language dflt;"));
    assert!(!is_single_or_alternate_sub(""));
}
