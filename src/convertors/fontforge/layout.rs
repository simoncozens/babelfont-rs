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

pub(crate) fn make_langsys(script: SmolStr, language: SmolStr) -> Vec<fea_rs_ast::Statement> {
    vec![
        fea_rs_ast::Statement::Script(fea_rs_ast::ScriptStatement::new(script.into())),
        fea_rs_ast::Statement::Language(fea_rs_ast::LanguageStatement::new(
            language.into(),
            true,
            false,
        )),
    ]
}
