use std::collections::{BTreeSet, HashMap, HashSet};

use indexmap::IndexMap;

use crate::features::PossiblyAutomaticCode;
use crate::filters::fontforge_anchor_lookups::{
    AnchorLookup, AnchorLookupKind, ANCHOR_CLASS_KEY, KEY,
};
use crate::filters::FontFilter;
use crate::{Anchor, Font};

/// The ufo2ft feature-writer list, read from a Glyphs file's userData or a UFO's lib.
const FEATURE_WRITERS_KEY: &str = "com.github.googlei18n.ufo2ft.featureWriters";
const GLYPHS_USER_DATA_KEY: &str = "com.schriftgestalt.Glyphs.userData";
const UFO_LIB_KEY: &str = "norad.lib";
/// The feature writers that build lookups from anchors.
const ANCHOR_WRITERS: [&str; 3] = [
    "MarkFeatureWriter",
    "ContextualMarkFeatureWriter",
    "CursFeatureWriter",
];

/// A filter that writes a FontForge source's anchor lookups (mark-to-base,
/// mark-to-ligature, mark-to-mark and cursive) as explicit feature code, the way
/// FontForge's exporter builds them, and turns off the compiler's mark and cursive
/// feature writers.
///
/// FontForge exports each anchor lookup as it is declared: one lookup per SFD
/// lookup, one subtable per SFD subtable, with the lookup's flags and feature
/// registrations. A subtable's bases are every glyph with a base-side anchor of one
/// of its anchor classes and its marks every glyph with a mark anchor of one, whatever
/// the glyph's GDEF class.
///
/// Anchor coordinates are written as they stand, rounded, so this runs after any
/// filter that moves anchors. It does nothing for a font with more than one master,
/// whose anchors would need variable positions, or for sources other than FontForge.
#[derive(Default)]
pub struct FontForgeMarkLookups;

impl FontForgeMarkLookups {
    /// Create a new FontForgeMarkLookups filter
    pub fn new() -> Self {
        FontForgeMarkLookups
    }
}

/// An anchor point of one glyph, as the SFD reader recorded it.
struct SfdAnchor<'a> {
    glyph: &'a str,
    class: &'a str,
    kind: &'a str,
    ligature_component: usize,
    anchor: &'a Anchor,
}

impl SfdAnchor<'_> {
    fn fea(&self) -> String {
        format!(
            "<anchor {} {}>",
            self.anchor.x.round() as i64,
            self.anchor.y.round() as i64
        )
    }
}

/// A glyph's entry and exit in a cursive subtable, as feature-file anchors.
#[derive(Default)]
struct CursiveAnchors {
    entry: Option<String>,
    exit: Option<String>,
}

/// A lookup definition to place among the feature prefixes.
struct Definition {
    /// The prefix to place it before, if it is still there.
    before: Option<String>,
    name: String,
    code: String,
}

/// A `languagesystem` statement. DFLT sorts first, and a script's dflt before its
/// other languages.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct LanguageSystem {
    not_default_script: bool,
    script: String,
    not_default_language: bool,
    language: String,
}

impl LanguageSystem {
    fn new(script: &str, language: &str) -> Self {
        LanguageSystem {
            not_default_script: script != "DFLT",
            script: script.to_string(),
            not_default_language: language != "dflt",
            language: language.to_string(),
        }
    }
}

/// The rules of one lookup, built from the anchors.
struct LookupWriter<'a> {
    anchors: &'a [SfdAnchor<'a>],
    mark_classes: &'a mut usize,
}

impl LookupWriter<'_> {
    /// The anchors of `kind` whose class is one of `classes`, grouped by glyph in
    /// glyph order.
    fn by_glyph<'b>(
        &'b self,
        classes: &[String],
        kind: &str,
    ) -> IndexMap<&'b str, Vec<&'b SfdAnchor<'b>>> {
        let mut out: IndexMap<&str, Vec<&SfdAnchor>> = IndexMap::new();
        for anchor in self.anchors {
            if anchor.kind == kind && classes.iter().any(|class| class == anchor.class) {
                out.entry(anchor.glyph).or_default().push(anchor);
            }
        }
        out
    }

    /// `markClass` statements for the marks of `classes`, and the class name each
    /// anchor class got. A mark glyph joins only the first of the lookup's classes
    /// it has an anchor in: one lookup cannot place a mark by two classes.
    fn mark_classes(
        &mut self,
        classes: &[String],
        lookup: &str,
        placed_marks: &mut HashSet<String>,
        lines: &mut Vec<String>,
    ) -> HashMap<String, String> {
        let mut names = HashMap::new();
        for class in classes {
            let marks: Vec<&SfdAnchor> = self
                .anchors
                .iter()
                .filter(|a| a.kind == "mark" && a.class == class)
                .collect();
            let mut statements = vec![];
            let name = format!("MC_{}", *self.mark_classes + 1);
            for mark in marks {
                if !placed_marks.insert(mark.glyph.to_string()) {
                    log::warn!(
                        "lookup {lookup:?}: mark {:?} already has a class in this lookup; \
                         its anchor class {class:?} is not used",
                        mark.glyph
                    );
                    continue;
                }
                statements.push(format!("markClass {} {} @{name};", mark.glyph, mark.fea()));
            }
            if !statements.is_empty() {
                *self.mark_classes += 1;
                lines.extend(statements);
                names.insert(class.clone(), name);
            }
        }
        names
    }

    /// The rules of one subtable.
    fn subtable_rules(
        &mut self,
        lookup: &AnchorLookup,
        classes: &[String],
        placed_marks: &mut HashSet<String>,
        mark_class_lines: &mut Vec<String>,
    ) -> Vec<String> {
        let mut rules = vec![];
        if lookup.kind == AnchorLookupKind::Cursive {
            let mut glyphs: IndexMap<&str, CursiveAnchors> = IndexMap::new();
            for anchor in self.anchors {
                if !classes.iter().any(|class| class == anchor.class) {
                    continue;
                }
                let slot = glyphs.entry(anchor.glyph).or_default();
                match anchor.kind {
                    "entry" if slot.entry.is_none() => slot.entry = Some(anchor.fea()),
                    "exit" if slot.exit.is_none() => slot.exit = Some(anchor.fea()),
                    _ => {}
                }
            }
            let null = || "<anchor NULL>".to_string();
            for (glyph, CursiveAnchors { entry, exit }) in glyphs {
                if entry.is_none() && exit.is_none() {
                    continue;
                }
                rules.push(format!(
                    "pos cursive {glyph} {} {};",
                    entry.unwrap_or_else(null),
                    exit.unwrap_or_else(null)
                ));
            }
            return rules;
        }
        let names = self.mark_classes(classes, &lookup.name, placed_marks, mark_class_lines);
        let attach = |anchors: &[&SfdAnchor]| -> Vec<String> {
            classes
                .iter()
                .filter_map(|class| {
                    let anchor = anchors.iter().find(|a| a.class == class)?;
                    Some(format!("{} mark @{}", anchor.fea(), names.get(class)?))
                })
                .collect()
        };
        let (keyword, base_kind) = match lookup.kind {
            AnchorLookupKind::MarkToBase => ("base", "basechar"),
            AnchorLookupKind::MarkToMark => ("mark", "basemark"),
            AnchorLookupKind::MarkToLigature => ("ligature", "baselig"),
            AnchorLookupKind::Cursive => unreachable!(),
        };
        for (glyph, anchors) in self.by_glyph(classes, base_kind) {
            if lookup.kind != AnchorLookupKind::MarkToLigature {
                let parts = attach(&anchors);
                if !parts.is_empty() {
                    rules.push(format!("pos {keyword} {glyph} {};", parts.join(" ")));
                }
                continue;
            }
            // A ligature has as many components as its highest component index says.
            let components = anchors
                .iter()
                .map(|a| a.ligature_component + 1)
                .max()
                .unwrap_or(0);
            let mut any = false;
            let parts: Vec<String> = (0..components)
                .map(|component| {
                    let on_component: Vec<&SfdAnchor> = anchors
                        .iter()
                        .copied()
                        .filter(|a| a.ligature_component == component)
                        .collect();
                    let parts = attach(&on_component);
                    if parts.is_empty() {
                        "<anchor NULL>".to_string()
                    } else {
                        any = true;
                        parts.join(" ")
                    }
                })
                .collect();
            if any {
                rules.push(format!(
                    "pos ligature {glyph} {};",
                    parts.join(" ligComponent ")
                ));
            }
        }
        rules
    }
}

/// The `lookupflag` statement for an SFD lookup flag, if it sets any of the four
/// named bits.
fn lookup_flag(name: &str, flag: u16) -> Option<String> {
    if flag & !0x000F != 0 {
        log::warn!(
            "lookup {name:?}: flag {flag:#06x} carries bits (mark-attachment class or \
             filtering set) that are not converted"
        );
    }
    let names: Vec<&str> = [
        (0x1, "RightToLeft"),
        (0x2, "IgnoreBaseGlyphs"),
        (0x4, "IgnoreLigatures"),
        (0x8, "IgnoreMarks"),
    ]
    .iter()
    .filter(|(bit, _)| flag & bit != 0)
    .map(|(_, flag_name)| *flag_name)
    .collect();
    (!names.is_empty()).then(|| format!("lookupflag {};", names.join(" ")))
}

/// Whether a feature file can spell the glyph name.
fn fea_can_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || b"._*+:^|~-".contains(&b))
}

/// Turn off the anchor feature writers in a ufo2ft feature-writer list stored under
/// `container` (a Glyphs userData or UFO lib dictionary), keeping the others.
fn disable_anchor_writers(font: &mut Font, container: &str) {
    let mut dict = font
        .format_specific
        .get(container)
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let writers = match dict.get(FEATURE_WRITERS_KEY).and_then(|v| v.as_array()) {
        Some(list) => list
            .iter()
            .filter(|entry| {
                !entry
                    .get("class")
                    .and_then(|c| c.as_str())
                    .is_some_and(|class| ANCHOR_WRITERS.contains(&class))
            })
            .cloned()
            .collect(),
        None => vec![
            serde_json::json!({"class": "KernFeatureWriter"}),
            serde_json::json!({"class": "GdefFeatureWriter"}),
        ],
    };
    dict.insert(
        FEATURE_WRITERS_KEY.to_string(),
        serde_json::Value::Array(writers),
    );
    font.format_specific
        .insert(container.to_string(), serde_json::Value::Object(dict));
}

/// Make sure every script and language the new registrations use has a
/// `languagesystem` statement, keeping DFLT first.
fn declare_language_systems(font: &mut Font, needed: &BTreeSet<LanguageSystem>) {
    let existing = font
        .features
        .prefixes
        .get("LanguageSystems")
        .map(|code| code.code.clone())
        .unwrap_or_default();
    let mut systems: Vec<LanguageSystem> = existing
        .lines()
        .filter_map(|line| {
            let mut words = line
                .trim()
                .trim_end_matches(';')
                .split_whitespace()
                .skip_while(|w| *w != "languagesystem")
                .skip(1);
            Some(LanguageSystem::new(words.next()?, words.next()?))
        })
        .collect();
    let missing: Vec<&LanguageSystem> = needed.iter().filter(|s| !systems.contains(s)).collect();
    if missing.is_empty() {
        return;
    }
    systems.extend(missing.into_iter().cloned());
    systems.sort();
    let code = systems
        .iter()
        .map(|system| format!("languagesystem {} {};", system.script, system.language))
        .collect::<Vec<_>>()
        .join("\n");
    match font.features.prefixes.get_mut("LanguageSystems") {
        Some(prefix) => prefix.code = code,
        None => {
            font.features.prefixes.insert_before(
                0,
                "LanguageSystems".into(),
                PossiblyAutomaticCode::new(code),
            );
        }
    }
}

impl FontFilter for FontForgeMarkLookups {
    fn apply(&self, font: &mut Font) -> Result<(), crate::BabelfontError> {
        let Some(lookups) = font.format_specific.get_parse_opt::<Vec<AnchorLookup>>(KEY) else {
            log::warn!("No FontForge anchor lookups were recorded; not writing any");
            return Ok(());
        };
        if font.masters.len() != 1 {
            log::warn!("Not writing FontForge anchor lookups for a font with several masters");
            return Ok(());
        }
        let master_id = font.masters[0].id.clone();
        let mut anchors = vec![];
        for glyph in font.glyphs.iter().filter(|g| g.exported) {
            if !fea_can_name(&glyph.name) {
                continue;
            }
            let Some(layer) = glyph.layers.iter().find(|layer| {
                layer.master == crate::LayerType::DefaultForMaster(master_id.clone())
            }) else {
                continue;
            };
            for anchor in &layer.anchors {
                let text = |key: &str| anchor.format_specific.get(key).and_then(|v| v.as_str());
                let (Some(class), Some(kind)) = (text(ANCHOR_CLASS_KEY), text("sfd.kind")) else {
                    continue;
                };
                anchors.push(SfdAnchor {
                    glyph: glyph.name.as_str(),
                    class,
                    kind,
                    ligature_component: anchor
                        .format_specific
                        .get("sfd.index")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as usize,
                    anchor,
                });
            }
        }

        let mut mark_class_count = 0;
        let mut definitions: Vec<Definition> = vec![];
        // feature -> (script, language) -> lookups
        let mut features: IndexMap<String, IndexMap<LanguageSystem, Vec<String>>> =
            IndexMap::new();
        let mut language_systems = BTreeSet::new();
        for lookup in &lookups {
            let mut writer = LookupWriter {
                anchors: &anchors,
                mark_classes: &mut mark_class_count,
            };
            let mut placed_marks = HashSet::new();
            let mut mark_class_lines = vec![];
            let mut body = vec![];
            for classes in &lookup.subtables {
                let rules = writer.subtable_rules(
                    lookup,
                    classes,
                    &mut placed_marks,
                    &mut mark_class_lines,
                );
                if rules.is_empty() {
                    continue;
                }
                if !body.is_empty() {
                    body.push("subtable;".to_string());
                }
                body.extend(rules);
            }
            if body.is_empty() {
                continue;
            }
            if let Some(flag) = lookup_flag(&lookup.name, lookup.flag) {
                body.insert(0, flag);
            }
            let mut code = mark_class_lines;
            code.push(format!("lookup {} {{", lookup.name));
            code.extend(body.iter().map(|line| format!("    {line}")));
            code.push(format!("}} {};", lookup.name));
            definitions.push(Definition {
                before: lookup.before.clone(),
                name: lookup.name.clone(),
                code: code.join("\n"),
            });
            for registration in &lookup.registrations {
                let system = LanguageSystem::new(&registration.script, &registration.language);
                features
                    .entry(registration.feature.clone())
                    .or_default()
                    .entry(system.clone())
                    .or_default()
                    .push(lookup.name.clone());
                language_systems.insert(system);
            }
        }

        for Definition { before, name, code } in definitions {
            let code = PossiblyAutomaticCode::new(code);
            match before.and_then(|b| font.features.prefixes.get_index_of(b.as_str())) {
                Some(index) => {
                    font.features.prefixes.insert_before(index, name.into(), code);
                }
                None => {
                    font.features.prefixes.insert(name.into(), code);
                }
            }
        }
        for (feature, systems) in features {
            let mut lines = vec![];
            for (system, names) in systems {
                lines.push(format!("script {};", system.script));
                lines.push(format!("language {};", system.language));
                lines.extend(names.iter().map(|name| format!("lookup {name};")));
            }
            let code = lines.join("\n");
            match font
                .features
                .features
                .iter_mut()
                .find(|(tag, _)| tag.as_str() == feature)
            {
                Some((_, existing)) => {
                    existing.code.push('\n');
                    existing.code.push_str(&code);
                }
                None => font
                    .features
                    .features
                    .push((feature.into(), PossiblyAutomaticCode::new(code))),
            }
        }
        declare_language_systems(font, &language_systems);
        disable_anchor_writers(font, GLYPHS_USER_DATA_KEY);
        disable_anchor_writers(font, UFO_LIB_KEY);
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(FontForgeMarkLookups::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("fontforgemarklookups")
            .long("fontforge-mark-lookups")
            .help(
                "Write a FontForge source's anchor lookups as feature code, one per SFD \
                 lookup with FontForge's coverage, and turn off the compiler's mark and \
                 cursive feature writers",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(all(test, feature = "fontforge"))]
mod tests {
    use super::*;

    fn glyph(name: &str, gid: usize, lines: &str) -> String {
        format!("StartChar: {name}\nEncoding: {gid} -1 {gid}\nWidth: 500\n{lines}EndChar\n")
    }

    fn sfd() -> String {
        [
            "SplineFontDB: 3.0\nFontName: T\nAscent: 800\nDescent: 200\n",
            "Lookup: 260 1 0 \"base\" {\"base-1\" \"base-2\"} ['mark' ('arab' <'dflt' > ) ]\n",
            "Lookup: 261 0 0 \"lig\" {\"lig-1\"} ['mark' ('arab' <'dflt' > ) ]\n",
            "Lookup: 262 0 0 \"mkmk\" {\"mkmk-1\"} ['mkmk' ('arab' <'dflt' > ) ]\n",
            "Lookup: 259 1 0 \"curs\" {\"curs-1\"} ['curs' ('DFLT' <'dflt' > ) ]\n",
            "AnchorClass2: \"Above\" \"base-1\" \"Below\" \"base-1\" \"Dot\" \"base-2\" \
             \"LigAbove\" \"lig-1\" \"MarkAbove\" \"mkmk-1\" \"Join\" \"curs-1\"\n",
            "BeginChars: 7 7\n",
            // No GDEF class: still a base of every anchor class it has.
            &glyph(
                "peh",
                0,
                "GlyphClass: 1\nAnchorPoint: \"Above\" 300 600 basechar 0\n\
                 AnchorPoint: \"Below\" 300.6 -200 basechar 0\nAnchorPoint: \"Dot\" 10 20 basechar 0\n",
            ),
            &glyph("beh", 1, "AnchorPoint: \"Below\" 250 -150 basechar 0\n"),
            &glyph(
                "fatha",
                2,
                "AnchorPoint: \"Above\" 100 500 mark 0\nAnchorPoint: \"LigAbove\" 100 500 mark 0\n\
                 AnchorPoint: \"MarkAbove\" 100 500 mark 0\nAnchorPoint: \"MarkAbove\" 100 800 basemark 0\n",
            ),
            &glyph("kasra", 3, "AnchorPoint: \"Below\" 90 -50 mark 0\n"),
            &glyph(
                "lam_alef",
                4,
                "AnchorPoint: \"LigAbove\" 400 700 baselig 0\nAnchorPoint: \"LigAbove\" 100 700 baselig 2\n",
            ),
            &glyph(
                "kashida",
                5,
                "AnchorPoint: \"Join\" 500 0 entry 0\nAnchorPoint: \"Join\" 0 0 exit 0\n",
            ),
            &glyph("dot", 6, "AnchorPoint: \"Dot\" 5 5 mark 0\n"),
            "EndChars\nEndSplineFont\n",
        ]
        .concat()
    }

    #[test]
    fn test_anchor_lookups_are_written_with_fontforge_coverage() {
        let mut font = crate::convertors::fontforge::load_str(&sfd()).unwrap();
        FontForgeMarkLookups::new().apply(&mut font).unwrap();
        let prefixes = &font.features.prefixes;
        assert_eq!(
            prefixes["base_"].code,
            "markClass fatha <anchor 100 500> @MC_1;\n\
             markClass kasra <anchor 90 -50> @MC_2;\n\
             markClass dot <anchor 5 5> @MC_3;\n\
             lookup base_ {\n    \
             lookupflag RightToLeft;\n    \
             pos base peh <anchor 300 600> mark @MC_1 <anchor 301 -200> mark @MC_2;\n    \
             pos base beh <anchor 250 -150> mark @MC_2;\n    \
             subtable;\n    \
             pos base peh <anchor 10 20> mark @MC_3;\n\
             } base_;"
        );
        assert_eq!(
            prefixes["lig"].code,
            "markClass fatha <anchor 100 500> @MC_4;\n\
             lookup lig {\n    \
             pos ligature lam_alef <anchor 400 700> mark @MC_4 ligComponent <anchor NULL> \
             ligComponent <anchor 100 700> mark @MC_4;\n\
             } lig;"
        );
        assert_eq!(
            prefixes["mkmk"].code,
            "markClass fatha <anchor 100 500> @MC_5;\n\
             lookup mkmk {\n    \
             pos mark fatha <anchor 100 800> mark @MC_5;\n\
             } mkmk;"
        );
        assert_eq!(
            prefixes["curs"].code,
            "lookup curs {\n    \
             lookupflag RightToLeft;\n    \
             pos cursive kashida <anchor 500 0> <anchor 0 0>;\n\
             } curs;"
        );
        let features: Vec<(String, String)> = font
            .features
            .features
            .iter()
            .map(|(tag, code)| (tag.to_string(), code.code.clone()))
            .collect();
        assert_eq!(
            features,
            vec![
                (
                    "mark".to_string(),
                    "script arab;\nlanguage dflt;\nlookup base_;\nlookup lig;".to_string()
                ),
                (
                    "mkmk".to_string(),
                    "script arab;\nlanguage dflt;\nlookup mkmk;".to_string()
                ),
                (
                    "curs".to_string(),
                    "script DFLT;\nlanguage dflt;\nlookup curs;".to_string()
                ),
            ]
        );
        assert_eq!(
            prefixes["LanguageSystems"].code,
            "languagesystem DFLT dflt;\nlanguagesystem arab dflt;"
        );
        let writers = &font.format_specific.get(GLYPHS_USER_DATA_KEY).unwrap()[FEATURE_WRITERS_KEY];
        assert_eq!(
            writers,
            &serde_json::json!([{"class": "KernFeatureWriter"}, {"class": "GdefFeatureWriter"}])
        );
    }
}
