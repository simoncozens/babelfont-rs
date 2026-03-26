use fea_rs_ast::{
    ChainedContextStatement, FeatureFile, GlyphClass, GlyphContainer, GlyphName,
    LanguageSystemStatement, LookupBlock, SubOrPos, ToplevelItem,
};
use indexmap::{IndexMap, IndexSet};
use skrifa::{
    FontRef, GlyphId, GlyphId16, GlyphNames, Tag,
    raw::{
        ReadError, TableProvider,
        tables::{
            gdef::Gdef,
            gpos::Gpos,
            gsub::{ChainedSequenceContext, Gsub, ReverseChainSingleSubstFormat1, SequenceContext},
            layout::CoverageTable,
        },
    },
};
use smol_str::SmolStr;

mod contextual;
mod gpos;
mod gsub;

struct UncompileContext<'a> {
    lookups: IndexMap<SmolStr, LookupBlock>,
    symbols: IndexMap<SmolStr, usize>,
    gpos: Option<Gpos<'a>>,
    gsub: Option<Gsub<'a>>,
    gdef: Option<Gdef<'a>>,
    language_systems: IndexMap<Tag, IndexSet<Tag>>,
    glyph_names: GlyphNames<'a>,
}

impl<'a> UncompileContext<'a> {
    fn new(font: &'a FontRef) -> Result<Self, ReadError> {
        let glyph_names = GlyphNames::new(font);
        Ok(Self {
            lookups: IndexMap::new(),
            symbols: IndexMap::new(),
            gpos: font.gpos().ok(),
            gsub: font.gsub().ok(),
            gdef: font.gdef().ok(),
            language_systems: IndexMap::new(),
            glyph_names,
        })
    }

    fn get_name(&self, id: GlyphId16) -> GlyphName {
        let str: SmolStr = self
            .glyph_names
            .get(GlyphId::new(id.to_u32()))
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("gid{:04}", id.to_u32()))
            .into();
        GlyphName::new(&str)
    }

    fn gather_language_systems(&mut self) -> Result<(), ReadError> {
        let mut systems = IndexMap::new();
        for script_list in [
            self.gsub.as_ref().and_then(|g| g.script_list().ok()),
            self.gpos.as_ref().and_then(|g| g.script_list().ok()),
        ]
        .into_iter()
        .flatten()
        {
            for script_record in script_list.script_records() {
                let script_tag = script_record.script_tag();
                let languages: &mut IndexSet<Tag> = systems.entry(script_tag).or_default();
                let script = script_record.script(script_list.offset_data())?;
                if script.default_lang_sys().is_some() {
                    languages.insert(Tag::new(b"dflt"));
                }
                for lang_sys in script.lang_sys_records() {
                    let lang_sys_tag = lang_sys.lang_sys_tag();
                    languages.insert(lang_sys_tag);
                }
            }
        }
        self.language_systems = systems;
        Ok(())
    }

    fn dump_language_systems(&self) -> Vec<ToplevelItem> {
        let mut items = vec![];
        for (script_tag, lang_sys_tags) in &self.language_systems {
            for lang_sys_tag in lang_sys_tags {
                let lss =
                    LanguageSystemStatement::new(script_tag.to_string(), lang_sys_tag.to_string());
                items.push(ToplevelItem::LanguageSystem(lss));
            }
        }
        items
    }

    fn resolve_coverage(&self, coverage: &CoverageTable) -> Vec<GlyphContainer> {
        coverage
            .iter()
            .map(|g| GlyphContainer::GlyphName(self.get_name(g)))
            .collect()
    }
    fn resolve_coverage_to_class(&self, coverage: &CoverageTable) -> GlyphContainer {
        let glyphs = self.resolve_coverage(coverage);
        if glyphs.len() == 1 {
            glyphs.into_iter().next().unwrap()
        } else {
            GlyphContainer::GlyphClass(GlyphClass::new(glyphs, 0..0))
        }
    }
    fn create_next_lookup_block(&mut self, prefix: &str) -> LookupBlock {
        let symbol_index = self.symbols.entry(prefix.into()).or_insert(1);
        let i = *symbol_index;
        let name = SmolStr::new(format!("{}_{}", prefix, i));
        *symbol_index += 1;
        LookupBlock::new(name.clone(), vec![], false, 0..0)
    }

    fn get_lookup_name(&self, lookup_list_index: u16) -> SmolStr {
        format!("lookup_{}", lookup_list_index).into() // This is WRONG but I want to make progress
    }
}

pub fn uncompile(font: &FontRef) -> Result<FeatureFile, ReadError> {
    let mut context = UncompileContext::new(font)?;
    context.gather_language_systems()?;
    let mut ff = FeatureFile::new(vec![]);
    ff.statements.extend(context.dump_language_systems());

    context.uncompile_gsub_lookups()?;
    context.uncompile_gpos_lookups()?;
    // Add all lookups to the feature file
    for lookup in context.lookups.values() {
        ff.statements.push(ToplevelItem::Lookup(lookup.clone()));
    }

    Ok(ff)
}

#[cfg(test)]
mod tests {
    use fea_rs_ast::AsFea;

    use super::*;
    #[test]
    fn test_uncompile_static() {
        let data = std::fs::read("resources/test.ttf").unwrap();
        let fontref = FontRef::new(&data).unwrap();
        let ff = uncompile(&fontref).unwrap();
        assert_eq!(ff.as_fea(""), "");
    }
}
