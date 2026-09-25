use indexmap::IndexMap;
use smol_str::SmolStr;

#[derive(Debug, Clone)]
pub(crate) enum LookupType {
    SingleSubstitution = 1, // fea_rs::SingleSubstStatement
    MultipleSubstitution,   // fea_rs::MultipleSubstStatement
    AlternateSubstitution,  // fea_rs::AlternateSubstStatement
    LigatureSubstitution,   // fea_rs::LigatureSubstStatement
    GsubContext,            // fea_rs::ChainedContextStatement (even though no lookahead/backtrack)
    GsubChainContext,       // fea_rs::ChainedContextStatement
    // 7 is extension, which is only an internal representation detail
    ReverseChain = 8,               //fea_rs::ReverseChainSingleSubstStatement
    SinglePosition = 0x101,         // fea_rs::SinglePosStatement
    PairPosition = 0x102,           // fea_rs::PairPosStatement
    CursivePosition = 0x103,        // fea_rs::CursivePosStatement
    MarkToBasePosition = 0x104,     // fea_rs::MarkBasePosStatement
    MarkToLigaturePosition = 0x105, // fea_rs::MarkLigPosStatement
    MarkToMarkPosition = 0x106,     // fea_rs::MarkMarkPosStatement
    ContextPosition = 0x107,        // fea_rs::ChainedContextStatement
    ChainContextPosition = 0x108,   // fea_rs::ChainedContextStatement
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct FeatureLangSys {
    pub(crate) feature: SmolStr,
    pub(crate) script: SmolStr,
    pub(crate) language: SmolStr,
}
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct LookupInfo {
    pub(crate) lookup_type: LookupType,
    pub(crate) flag: u16, // This will become a fea_rs::LookupFlagStatement once we build the lookup
    pub(crate) features: Vec<FeatureLangSys>,
    pub(crate) block: fea_rs_ast::LookupBlock,
    pub(crate) subtables: IndexMap<SmolStr, Vec<fea_rs_ast::Statement>>,
}

#[derive(Debug, Clone)]
pub(crate) struct GTable(pub IndexMap<String, LookupInfo>);

impl GTable {
    pub(crate) fn find_subtable_mut(
        &mut self,
        name: &SmolStr,
    ) -> Option<&mut Vec<fea_rs_ast::Statement>> {
        let info = self
            .0
            .values_mut()
            .find(|info| info.subtables.contains_key(name))?;
        info.subtables.get_mut(name)
    }
}

pub(crate) fn make_ligature_statement(
    components: &[SmolStr],
    ligature: &SmolStr,
) -> fea_rs_ast::LigatureSubstStatement {
    fea_rs_ast::LigatureSubstStatement::new(
        components
            .iter()
            .map(|c| fea_rs_ast::GlyphContainer::GlyphName(fea_rs_ast::GlyphName::new(c)))
            .collect(),
        fea_rs_ast::GlyphContainer::GlyphName(fea_rs_ast::GlyphName::new(ligature)),
        vec![],
        vec![],
        0..0,
        false,
    )
}

/// One position of a chain/context rule.
#[derive(Debug, Clone)]
pub(crate) enum GlyphGroup {
    /// An explicit coverage table, glyph list, or class.
    Glyphs(Vec<String>),
    /// FontForge's implicit class 0, "All_Others": every glyph in the font that is
    /// not in any of the sibling classes. It is never written out in the SFD, so it
    /// can only be expanded once the full glyph list is known.
    AllOthers(Vec<Vec<String>>),
}

impl GlyphGroup {
    /// Expand to the concrete glyph names, given every glyph in the font.
    ///
    /// The result can be empty even when the group is not: `AllOthers` is empty
    /// whenever its sibling classes between them cover the whole font. Emptiness
    /// is therefore only knowable after expansion, and is the caller's to handle.
    pub(crate) fn resolve(&self, all_glyphs: &[String]) -> Vec<String> {
        match self {
            GlyphGroup::Glyphs(g) => g.clone(),
            GlyphGroup::AllOthers(siblings) => {
                let named: std::collections::HashSet<&String> = siblings.iter().flatten().collect();
                all_glyphs
                    .iter()
                    .filter(|g| !named.contains(g))
                    .cloned()
                    .collect()
            }
        }
    }
}

/// Names the glyph classes a class-kind lookup uses, so the feature file declares
/// each list once and every position that wants it refers to the name.
///
/// FontForge's own SFD-to-FEA export does the same. Only the class kind gets this:
/// a coverage or glyph section lists its glyphs per rule, so there is no class to
/// name. Lists are keyed by content, so a class used as input and again as
/// backtrack is declared once.
#[derive(Debug, Default)]
pub(crate) struct ClassNamer {
    by_glyphs: IndexMap<Vec<String>, SmolStr>,
}

impl ClassNamer {
    /// How to refer to `glyphs`: a single glyph is written bare, anything longer
    /// gets an `@class`, declared on first use. Keyed by content, so a class used
    /// as input and again as backtrack is declared once.
    pub(crate) fn reference(
        &mut self,
        glyphs: &[String],
        lookup: &str,
    ) -> fea_rs_ast::GlyphContainer {
        if let [only] = glyphs {
            return fea_rs_ast::GlyphContainer::GlyphName(fea_rs_ast::GlyphName::new(only));
        }
        // The container's string carries the leading `@`; the definition's name
        // does not. That is the crate's own convention.
        if let Some(name) = self.by_glyphs.get(glyphs) {
            return fea_rs_ast::GlyphContainer::GlyphClassName(SmolStr::from(format!("@{name}")));
        }
        let name = SmolStr::from(format!("{lookup}_c{}", self.by_glyphs.len() + 1));
        self.by_glyphs.insert(glyphs.to_vec(), name.clone());
        fea_rs_ast::GlyphContainer::GlyphClassName(SmolStr::from(format!("@{name}")))
    }

    /// The `@name = [...];` definitions, in first-use order.
    pub(crate) fn definitions(&self) -> Vec<fea_rs_ast::Statement> {
        self.by_glyphs
            .iter()
            .map(|(glyphs, name)| {
                let members = glyphs
                    .iter()
                    .map(|n| fea_rs_ast::GlyphContainer::GlyphName(fea_rs_ast::GlyphName::new(n)))
                    .collect();
                fea_rs_ast::Statement::GlyphClassDefinition(fea_rs_ast::GlyphClassDefinition::new(
                    name.to_string(),
                    fea_rs_ast::GlyphClass::new(members, 0..0),
                    0..0,
                ))
            })
            .collect()
    }
}

/// The counts an FPST section header declares, used to check the parsed body.
///
/// Class counts include FontForge's implicit class 0, which is never written out,
/// so a declared count of `n` corresponds to `n - 1` `Class:` lines.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FpstHeaderCounts {
    pub(crate) classes: usize,
    pub(crate) backtrack_classes: usize,
    pub(crate) lookahead_classes: usize,
    pub(crate) rules: usize,
}

/// What the body of an FPST section turned out to contain.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FpstBodyCounts {
    pub(crate) classes: usize,
    pub(crate) backtrack_classes: usize,
    pub(crate) lookahead_classes: usize,
    pub(crate) rules: usize,
}

impl FpstHeaderCounts {
    /// Describe every way `body` disagrees with what this header declared.
    ///
    /// Empty when the section is consistent. A disagreement means the body was
    /// misread, which matters here: contextual rules going missing quietly is the
    /// failure this parser exists to prevent.
    pub(crate) fn mismatches(&self, body: &FpstBodyCounts) -> Vec<String> {
        let mut out = Vec::new();
        let mut check = |what: &str, declared: usize, parsed: usize| {
            // Class 0 is implicit and never written, so `n` declared means `n - 1` lines.
            if declared.saturating_sub(1) != parsed {
                out.push(format!(
                    "declares {declared} {what} but its body has {parsed}"
                ));
            }
        };
        check("classes", self.classes, body.classes);
        check(
            "backtrack classes",
            self.backtrack_classes,
            body.backtrack_classes,
        );
        check(
            "lookahead classes",
            self.lookahead_classes,
            body.lookahead_classes,
        );
        if self.rules != body.rules {
            out.push(format!(
                "declares {} rules but its body has {}",
                self.rules, body.rules
            ));
        }
        out
    }
}

/// Whether a chain/context rule substitutes or positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuleKind {
    Sub,
    Pos,
}

/// How an FPST section spells the glyphs at each position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SfdKind {
    /// Every position carries its own coverage table.
    Coverage,
    /// Every position names literal glyphs, and each glyph takes its own marker.
    Glyph,
    /// Positions reference classes declared once for the whole section.
    Class,
}

impl SfdKind {
    pub(crate) fn parse(kind: &str) -> Option<Self> {
        match kind {
            "coverage" => Some(SfdKind::Coverage),
            "glyph" => Some(SfdKind::Glyph),
            "class" => Some(SfdKind::Class),
            _ => None,
        }
    }
}

/// Parsed data from a FontForge ChainSub2/ChainPos2 section.
#[derive(Debug, Clone)]
pub(crate) struct ChainPosSubEntry {
    pub(crate) kind: RuleKind,
    pub(crate) sfd_kind: SfdKind,
    /// The input sequence glyph groups (each is a coverage group, glyph list or class)
    pub(crate) matches: Vec<GlyphGroup>,
    /// Backtrack glyph groups, as the section stored them: a class section lists
    /// them farthest from the input first, a coverage or glyph section nearest first
    pub(crate) backtracks: Vec<GlyphGroup>,
    /// Lookahead glyph groups
    pub(crate) lookaheads: Vec<GlyphGroup>,
    /// Lookups to apply at each input position: position -> [lookup names]
    pub(crate) lookups: IndexMap<usize, Vec<String>>,
}

/// The `script` and `language` statements that register the next lookup for one
/// language system.
///
/// A FontForge lookup applies only to the scripts and languages it names: a
/// lookup registered for `latn/dflt` and not for `latn/TRK` does not run for
/// Turkish. In a feature file a `language` statement inherits the lookups
/// already registered for the script's default language unless it says
/// `exclude_dflt`, so every language other than `dflt` excludes them.
pub(crate) fn make_langsys(script: SmolStr, language: SmolStr) -> Vec<fea_rs_ast::Statement> {
    let include_dflt = language == "dflt";
    vec![
        fea_rs_ast::Statement::Script(fea_rs_ast::ScriptStatement::new(script.into())),
        fea_rs_ast::Statement::Language(fea_rs_ast::LanguageStatement::new(
            language.into(),
            include_dflt,
            false,
        )),
    ]
}
