//! Rust port of the `ufomerge` Python test suite (see `ufomerge/tests/`), adapted to
//! operate on `babelfont::Font` objects instead of UFOs.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::expect_used)]

use std::collections::HashSet;

use babelfont::{Anchor, Component, DecomposedAffine, Font, Shape, SmolStr};

use crate::{
    args::{DuplicateLookupHandling, ExistingGlyphHandling, LayoutHandling},
    create_test_font, fontmerge, fontsubset,
    glyphset::GlyphsetFilter,
};

fn glyph_names(font: &Font) -> HashSet<String> {
    font.glyphs.iter().map(|g| g.name.to_string()).collect()
}

fn assert_glyphset(font: &Font, expected: &[&str]) {
    let actual = glyph_names(font);
    let expected: HashSet<String> = expected.iter().map(|s| s.to_string()).collect();
    assert_eq!(actual, expected);
}

fn merge(
    mut font1: Font,
    font2: Font,
    include: Vec<&str>,
    exclude: Vec<&str>,
    codepoints: Vec<char>,
    existing: ExistingGlyphHandling,
) -> Font {
    let include: Vec<SmolStr> = include.into_iter().map(SmolStr::from).collect();
    let exclude: Vec<SmolStr> = exclude.into_iter().map(SmolStr::from).collect();
    let filter = GlyphsetFilter::new(include, exclude, codepoints, &mut font1, &font2, existing);
    fontmerge(
        font1,
        font2,
        filter,
        LayoutHandling::Subset,
        DuplicateLookupHandling::First,
        true,
        true,
    )
    .expect("merge failed")
}

fn merge_glyphs(
    font1: Font,
    font2: Font,
    include: Vec<&str>,
    existing: ExistingGlyphHandling,
) -> Font {
    merge(font1, font2, include, vec![], vec![], existing)
}

fn merge_glyphs_with_duplicate_policy(
    mut font1: Font,
    font2: Font,
    include: Vec<&str>,
    duplicate_lookups: DuplicateLookupHandling,
) -> Font {
    let include: Vec<SmolStr> = include.into_iter().map(SmolStr::from).collect();
    let filter = GlyphsetFilter::new(
        include,
        vec![],
        vec![],
        &mut font1,
        &font2,
        ExistingGlyphHandling::Replace,
    );
    fontmerge(
        font1,
        font2,
        filter,
        LayoutHandling::Subset,
        duplicate_lookups,
        true,
        true,
    )
    .expect("merge failed")
}

fn subset(font: Font, include: Vec<&str>, layout_handling: LayoutHandling) -> Font {
    let donor = font.clone();
    let include: Vec<SmolStr> = include.into_iter().map(SmolStr::from).collect();
    let mut target = font.clone();
    target.features = babelfont::Features::default();
    target.glyphs = babelfont::GlyphList::default();
    let filter =
        GlyphsetFilter::new_from_glyphs(include, &mut target, &donor, ExistingGlyphHandling::Skip);
    fontsubset(font, filter, layout_handling, true, true).expect("subset failed")
}

fn width(font: &Font, glyph: &str) -> f32 {
    font.glyphs.get(glyph).unwrap().layers[0].width
}

fn fea(font: &Font) -> String {
    font.features.to_fea()
}

#[test]
fn test_glyphset() {
    let font1 = create_test_font(vec!["A".into(), "B".into()], "");
    let font2 = create_test_font(vec!["C".into(), "D".into()], "");
    let merged = merge_glyphs(font1, font2, vec![], ExistingGlyphHandling::Replace);
    assert_glyphset(&merged, &["A", "B", "C", "D"]);
}

#[test]
fn test_component_closure() {
    let font1 = create_test_font(vec!["A".into(), "B".into()], "");
    let mut font2 = create_test_font(vec!["C".into(), "D".into(), "comp".into()], "");
    font2.glyphs.get_mut("D").unwrap().layers[0]
        .shapes
        .push(Shape::Component(Component {
            reference: "comp".into(),
            transform: DecomposedAffine::default(),
            location: Default::default(),
            format_specific: Default::default(),
        }));

    let merged = merge_glyphs(font1, font2, vec!["D"], ExistingGlyphHandling::Replace);
    assert_glyphset(&merged, &["A", "B", "D", "comp"]);
}

#[test]
fn test_kerning_flat() {
    let font1 = create_test_font(vec!["A".into(), "B".into()], "");
    let mut font2 = create_test_font(vec!["C".into(), "D".into(), "E".into()], "");
    font2.masters[0]
        .kerning
        .insert(("C".into(), "D".into()), 20);
    font2.masters[0]
        .kerning
        .insert(("C".into(), "E".into()), 15);
    font2.masters[0]
        .kerning
        .insert(("C".into(), "A".into()), -20);

    let merged = merge_glyphs(font1, font2, vec!["C", "D"], ExistingGlyphHandling::Replace);
    let kerning = &merged.masters[0].kerning;
    assert_eq!(
        kerning.get(&(SmolStr::from("C"), SmolStr::from("D"))),
        Some(&20)
    );
    assert_eq!(
        kerning.get(&(SmolStr::from("C"), SmolStr::from("A"))),
        Some(&-20)
    );
    assert_eq!(kerning.len(), 2);
}

#[test]
fn test_existing_handling() {
    let mut font1 = create_test_font(vec!["A".into(), "B".into()], "");
    font1.glyphs.get_mut("B").unwrap().layers[0].width = 100.0;
    let mut font2 = create_test_font(vec!["B".into(), "C".into()], "");
    font2.glyphs.get_mut("B").unwrap().layers[0].width = 200.0;

    let skipped = merge_glyphs(
        font1.clone(),
        font2.clone(),
        vec![],
        ExistingGlyphHandling::Skip,
    );
    assert_eq!(width(&skipped, "B"), 100.0);

    let replaced = merge_glyphs(font1, font2, vec![], ExistingGlyphHandling::Replace);
    assert_eq!(width(&replaced, "B"), 200.0);
}

#[test]
fn test_kerning_groups() {
    let mut font1 = create_test_font(vec!["A".into(), "B".into()], "");
    font1
        .first_kern_groups
        .insert("foo".into(), vec!["A".into()]);
    font1
        .second_kern_groups
        .insert("foo".into(), vec!["A".into()]);
    font1.masters[0]
        .kerning
        .insert(("@foo".into(), "@foo".into()), 10);
    font1.masters[0]
        .kerning
        .insert(("@foo".into(), "B".into()), 20);
    font1.masters[0]
        .kerning
        .insert(("A".into(), "@foo".into()), 30);
    font1.masters[0]
        .kerning
        .insert(("A".into(), "A".into()), 40);

    let mut font2 = create_test_font(vec!["A".into(), "B".into()], "");
    font2
        .first_kern_groups
        .insert("bar".into(), vec!["A".into()]);
    font2
        .second_kern_groups
        .insert("bar".into(), vec!["A".into()]);
    font2.masters[0]
        .kerning
        .insert(("@bar".into(), "@bar".into()), 50);
    font2.masters[0]
        .kerning
        .insert(("@bar".into(), "B".into()), 60);
    font2.masters[0]
        .kerning
        .insert(("A".into(), "@bar".into()), 70);
    font2.masters[0]
        .kerning
        .insert(("A".into(), "A".into()), 80);

    let merged = merge_glyphs(font1, font2, vec![], ExistingGlyphHandling::Replace);
    let kerning = &merged.masters[0].kerning;
    assert_eq!(
        kerning.get(&(SmolStr::from("@bar"), SmolStr::from("@bar"))),
        Some(&50)
    );
    assert_eq!(
        kerning.get(&(SmolStr::from("@bar"), SmolStr::from("B"))),
        Some(&60)
    );
    assert_eq!(
        kerning.get(&(SmolStr::from("A"), SmolStr::from("@bar"))),
        Some(&70)
    );
    assert_eq!(
        kerning.get(&(SmolStr::from("A"), SmolStr::from("A"))),
        Some(&80)
    );
    assert_eq!(kerning.len(), 4);
}

#[test]
fn test_dotted_circle() {
    fn dotted_circle_with_anchor(name: &str, x: f64, y: f64) -> Font {
        let mut font = create_test_font(vec!["A".into(), "B".into(), "dottedCircle".into()], "");
        font.glyphs.get_mut("dottedCircle").unwrap().layers[0]
            .anchors
            .push(Anchor {
                x,
                y,
                name: name.to_string(),
                format_specific: Default::default(),
            });
        font
    }

    let font1 = dotted_circle_with_anchor("top", 0.0, 100.0);
    let font2 = dotted_circle_with_anchor("bottom", 0.0, -100.0);
    let mut merged = merge(
        font1,
        font2,
        vec![],
        vec![],
        vec![],
        ExistingGlyphHandling::Replace,
    );
    merged.masters[0].kerning.clear(); // not relevant here
    let anchors: HashSet<String> = merged.glyphs.get("dottedCircle").unwrap().layers[0]
        .anchors
        .iter()
        .map(|a| a.name.clone())
        .collect();
    assert_eq!(
        anchors,
        HashSet::from(["top".to_string(), "bottom".to_string()])
    );
}

#[test]
fn test_exclude_glyphs_takes_precedence_over_codepoints() {
    // exclude_glyphs should win even if the excluded glyph also matches a requested codepoint,
    // and should leave the host's own glyph of the same name completely untouched.
    let mut font1 = create_test_font(vec!["A".into(), "B".into()], "");
    {
        let b1 = font1.glyphs.get_mut("B").unwrap();
        b1.layers[0].width = 100.0;
        b1.codepoints = vec![0x42];
    }
    let mut font2 = create_test_font(vec!["B".into(), "D".into()], "");
    {
        let b2 = font2.glyphs.get_mut("B").unwrap();
        b2.layers[0].width = 200.0;
        b2.codepoints = vec![0x42];
    }
    font2.glyphs.get_mut("D").unwrap().codepoints = vec![0x44];

    let merged = merge(
        font1,
        font2,
        vec![],
        vec!["B"],
        vec!['B', 'D'],
        ExistingGlyphHandling::Replace,
    );
    assert_glyphset(&merged, &["A", "B", "D"]);
    assert_eq!(width(&merged, "B"), 100.0);
    assert_eq!(merged.glyphs.get("B").unwrap().codepoints, vec![0x42]);
}

#[test]
fn test_glyphorder() {
    let font1 = create_test_font(vec!["A".into(), "B".into(), "C".into()], "");
    let font2 = create_test_font(vec!["D".into(), "E".into()], "");
    let merged = merge_glyphs(font1, font2, vec![], ExistingGlyphHandling::Replace);
    let order: Vec<String> = merged.glyphs.iter().map(|g| g.name.to_string()).collect();
    assert_eq!(order, vec!["A", "B", "C", "D", "E"]);
}

// --- Layout tests, ported from ufomerge/tests/test_layout.py ---

#[test]
fn test_layout_closure() {
    let font2 = create_test_font(
        vec!["A".into(), "B".into(), "C".into()],
        "feature ccmp { sub A A' B' by C; } ccmp;",
    );

    let ignored = subset(font2.clone(), vec!["A"], LayoutHandling::Ignore);
    assert_glyphset(&ignored, &["A"]);
    assert_eq!(fea(&ignored).trim(), "");

    let closed = subset(font2, vec!["A", "B"], LayoutHandling::Closure);
    assert_glyphset(&closed, &["A", "B", "C"]);
}

#[test]
fn test_ignorable_rule() {
    let font2 = create_test_font(
        vec!["A".into(), "B".into(), "C".into(), "D".into(), "E".into()],
        "lookup ccmp1 { sub A B by C; sub A D by E; } ccmp1; feature ccmp { lookup ccmp1; } ccmp;",
    );

    let subsetted = subset(font2.clone(), vec!["A", "B"], LayoutHandling::Subset);
    assert_glyphset(&subsetted, &["A", "B"]);

    let closed = subset(font2, vec!["A", "B"], LayoutHandling::Closure);
    assert_glyphset(&closed, &["A", "B", "C"]);
    let text = fea(&closed);
    assert!(text.contains("sub A B by C"));
    assert!(!text.contains("sub A D by E"));
}

#[test]
fn test_pos() {
    let font2 = create_test_font(
        vec!["A".into(), "B".into()],
        "lookup kern1 { pos [A B] 120; } kern1; feature kern { lookup kern1; } kern;",
    );

    let subsetted = subset(font2, vec!["A"], LayoutHandling::Subset);
    let text = fea(&subsetted);
    assert!(text.contains("pos [A] 120") || text.contains("pos A 120"));
    assert!(!text.contains("pos [A B]"));
}

#[test]
fn test_chain() {
    let font2 = create_test_font(
        vec!["A".into(), "B".into(), "C".into()],
        "lookup chained { pos A 120; pos B 200; } chained;\n\
         lookup chain { pos [A B]' lookup chained [A B C]; } chain;\n\
         feature kern { lookup chain; } kern;",
    );

    let subsetted = subset(font2, vec!["A", "C"], LayoutHandling::Subset);
    assert_glyphset(&subsetted, &["A", "C"]);
    let text = fea(&subsetted);
    assert!(text.contains("pos A 120"));
    assert!(!text.contains("pos B 200"));
}

#[test]
fn test_languagesystems() {
    let mut font1 = create_test_font(
        vec!["ka-deva".into(), "sa-deva".into()],
        "languagesystem latn dflt;\nfeature ccmp { sub A by B; } ccmp;",
    );
    // A and B aren't in font1's glyphset in this reduced test, but that's fine: we only care
    // about languagesystem merging here.
    font1.glyphs.push(babelfont::Glyph {
        name: "A".into(),
        ..Default::default()
    });
    font1.glyphs.push(babelfont::Glyph {
        name: "B".into(),
        ..Default::default()
    });

    let font2 = create_test_font(
        vec![
            "ka-deva".into(),
            "sa-deva".into(),
            "ta-deva".into(),
            "kssa-deva".into(),
            "la-deva".into(),
        ],
        "languagesystem DFLT dflt;\n\
         languagesystem dev2 dflt;\n\
         languagesystem dev2 NEP;\n\
         feature ccmp {\n\
           sub ka-deva by sa-deva;\n\
           script dev2;\n\
           language NEP;\n\
           sub ta-deva by kssa-deva;\n\
           sub la-deva by kssa-deva;\n\
         } ccmp;",
    );

    let merged = merge_glyphs(
        font1,
        font2,
        vec!["ka-deva", "sa-deva", "kssa-deva", "ta-deva"],
        ExistingGlyphHandling::Replace,
    );
    let text = fea(&merged);
    assert!(text.contains("languagesystem DFLT dflt"));
    assert!(text.contains("languagesystem latn dflt"));
    assert!(text.contains("languagesystem dev2 dflt"));
    assert!(text.contains("languagesystem dev2 NEP"));
    assert!(text.contains("sub A by B"));
    assert!(text.contains("sub ka-deva by sa-deva"));
    assert!(text.contains("sub ta-deva by kssa-deva"));
    assert!(!text.contains("sub la-deva by kssa-deva"));
}

#[test]
fn test_drop_contextual_empty_class() {
    let font2 = create_test_font(
        vec![
            "dagesh-hb".into(),
            "period".into(),
            "vav-hb".into(),
            "zayin-hb".into(),
        ],
        "@DAGESH = [dagesh-hb];\n\
         @OFFENDING_PUNCTUATION = [period];\n\
         lookup hebrew_mark_resolve_clashing_punctuation {\n\
           lookupflag RightToLeft;\n\
           pos [vav-hb zayin-hb] @DAGESH @OFFENDING_PUNCTUATION' 60;\n\
         } hebrew_mark_resolve_clashing_punctuation;\n\
         feature kern {\n\
           lookup hebrew_mark_resolve_clashing_punctuation;\n\
         } kern;",
    );

    let subsetted = subset(font2, vec!["period"], LayoutHandling::Subset);
    let text = fea(&subsetted);
    assert!(!text.contains("hebrew_mark_resolve_clashing_punctuation {"));
}

#[test]
fn test_deduplicate_classes() {
    let font2 = create_test_font(
        vec![
            "a".into(),
            "b".into(),
            "c".into(),
            "a.alt".into(),
            "b.alt".into(),
            "c.alt".into(),
        ],
        "@SOMETHING = [a b c];\n\
         @SOMETHING_ALT = [a.alt b.alt c.alt];\n\
         feature kern {\n\
           pos @SOMETHING @SOMETHING_ALT 60;\n\
           pos @SOMETHING_ALT @SOMETHING 30;\n\
         } kern;\n\
         feature locl {\n\
           sub @SOMETHING by @SOMETHING_ALT;\n\
         } locl;\n\
         feature rlig {\n\
           lookup bla {\n\
             @FOO = [b c];\n\
             sub @FOO a' @SOMETHING by b.alt;\n\
           } bla;\n\
         } rlig;",
    );

    let subsetted = subset(
        font2,
        vec!["a", "a.alt", "b.alt", "c", "c.alt"],
        LayoutHandling::Subset,
    );
    let text = fea(&subsetted);
    assert!(text.contains("feature kern"));
    assert!(text.contains("feature locl"));
    assert!(text.contains("feature rlig"));
    assert!(
        !text.contains(" b "),
        "glyph 'b' should have been subsetted out: {text}"
    );
}

#[test]
fn test_duplicate_lookup_names() {
    // Both fonts define a lookup called `kern1`. Merging should honour the default
    // ("first") policy: the host font's lookup wins and the donor's same-named
    // lookup is dropped, rather than both being welded together (which would
    // produce two lookups sharing a name and an uncompilable feature file).
    let font1 = create_test_font(
        vec!["A".into(), "B".into()],
        "lookup kern1 { pos A B -50; } kern1;\nfeature kern { lookup kern1; } kern;",
    );
    let font2 = create_test_font(
        vec!["C".into(), "D".into()],
        "lookup kern1 { pos C D -30; } kern1;\nfeature kern { lookup kern1; } kern;",
    );

    let merged = merge_glyphs(font1, font2, vec!["C", "D"], ExistingGlyphHandling::Replace);
    let text = fea(&merged);

    assert_eq!(
        text.matches("lookup kern1 {").count(),
        1,
        "only one definition of the duplicated lookup should remain:\n{text}"
    );
    assert!(
        text.contains("pos A B"),
        "the host's lookup should be the survivor:\n{text}"
    );
    assert!(
        !text.contains("pos C D"),
        "the donor's duplicate lookup should have been dropped:\n{text}"
    );
}

#[test]
fn test_duplicate_class_names() {
    // Both fonts define a glyph class called `@FOO`. FEA classes are unscoped
    // globals, so simply concatenating the two feature files would define `@FOO`
    // twice. The donor's definition should be renamed with a numeric suffix, and
    // its references updated to match, leaving the host's name untouched.
    let font1 = create_test_font(
        vec!["A".into(), "B".into()],
        "@FOO = [A B];\nfeature one { pos @FOO B -50; } one;",
    );
    let font2 = create_test_font(
        vec!["C".into(), "D".into()],
        "@FOO = [C D];\nfeature two { pos @FOO D -30; } two;",
    );

    let merged = merge_glyphs(font1, font2, vec!["C", "D"], ExistingGlyphHandling::Replace);
    let text = fea(&merged);

    assert_eq!(
        text.matches("@FOO = ").count(),
        1,
        "only one definition of @FOO should remain:\n{text}"
    );
    assert!(
        text.contains("@FOO = [A B];"),
        "the host's class keeps its name:\n{text}"
    );
    assert!(
        text.contains("@FOO_2 = [C D];"),
        "the donor's class is renamed:\n{text}"
    );
    assert!(
        text.contains("pos @FOO B -50;"),
        "the host's reference is unchanged:\n{text}"
    );
    assert!(
        text.contains("pos @FOO_2 D -30;"),
        "the donor's reference is updated:\n{text}"
    );
}

#[test]
fn test_duplicate_lookup_names_both() {
    // Under the "both" policy the donor's duplicate lookup is renamed rather than
    // dropped (and its references updated), so both lookups survive distinctly.
    let font1 = create_test_font(
        vec!["A".into(), "B".into()],
        "lookup kern1 { pos A B -50; } kern1;\nfeature kern { lookup kern1; } kern;",
    );
    let font2 = create_test_font(
        vec!["C".into(), "D".into()],
        "lookup kern1 { pos C D -30; } kern1;\nfeature kern { lookup kern1; } kern;",
    );

    let merged = merge_glyphs_with_duplicate_policy(
        font1,
        font2,
        vec!["C", "D"],
        DuplicateLookupHandling::Both,
    );
    let text = fea(&merged);

    assert!(
        text.contains("lookup kern1 {"),
        "the host's lookup keeps its name:\n{text}"
    );
    assert!(
        text.contains("lookup kern1_2 {"),
        "the donor's lookup is renamed:\n{text}"
    );
    assert!(
        text.contains("pos A B"),
        "the host's lookup content survives:\n{text}"
    );
    assert!(
        text.contains("pos C D"),
        "the donor's lookup content survives:\n{text}"
    );
    assert!(
        text.contains("lookup kern1_2;"),
        "the donor's reference is updated to the renamed lookup:\n{text}"
    );
}

// --- Known gaps: features present in ufomerge but not yet ported to fontmerge ---

#[test]
#[ignore = "GlyphsetFilter has no per-glyph existing_handling policy (see glyphset.rs::policy)"]
fn test_existing_handling_dict() {
    unimplemented!()
}
