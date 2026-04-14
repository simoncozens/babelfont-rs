//! Uncompile a TTF font into a fea file.
use std::collections::{HashMap, HashSet};

/// A handle to the version of fea-rs-ast that sr-eaf is using.
///
/// The return value of uncompile() will be a [fea_rs_ast::FeatureFile]; you will probably want to call `.as_fea()` on it.
pub use fea_rs_ast;
use fea_rs_ast::{
    Anchor, GlyphClass, GlyphClassDefStatement, GlyphClassDefinition, GlyphContainer, GlyphName,
    LanguageSystemStatement, LookupBlock, LookupReferenceStatement, MarkClass, MarkClassDefinition,
    Pos, Statement, SubOrPos, Subst, ToplevelItem,
};
use indexmap::{IndexMap, IndexSet};
/// A handle to the version of Skrifa that sr-eaf is using. Pass a skrifa::FontRef to uncompile()
pub use skrifa;
use skrifa::{
    raw::{
        tables::{
            gdef::Gdef,
            gpos::Gpos,
            gsub::{ClassDef, Gsub},
            layout::CoverageTable,
        },
        ReadError, TableProvider,
    },
    GlyphId, GlyphId16, GlyphNames, Tag,
};
use smol_str::SmolStr;

mod contextual;
mod gpos;
mod gsub;

const PROMOTE_TO_NAMED_CLASS_THRESHOLD: usize = 5;

/// The context object that holds all the information we need when uncompiling a font.
pub struct UncompileContext<'a> {
    /// All the lookups we have uncompiled so far, keyed by their name.
    pub lookups: IndexMap<SmolStr, LookupBlock>,
    pub lookup_map: HashMap<(String, u16), SmolStr>,
    symbols: IndexMap<SmolStr, usize>,
    gpos: Option<Gpos<'a>>,
    gsub: Option<Gsub<'a>>,
    gdef: Option<Gdef<'a>>,
    /// A mapping from script tags to the language system tags that are present in the font.
    pub language_systems: IndexMap<Tag, IndexSet<Tag>>,
    glyph_names: GlyphNames<'a>,
    /// Anchors on a glyph which we haven't worked out what they should be called.
    pub unnamed_anchors: IndexMap<SmolStr, Vec<Anchor>>,
    /// Anchors on a glyph which we have worked out what they should be called.
    pub anchors: IndexMap<SmolStr, IndexMap<SmolStr, Anchor>>,
    /// Mark classes, indexed by class name.
    pub mark_classes: IndexMap<SmolStr, Vec<MarkClassDefinition>>,
    /// Named glyph classes, indexed by class name.
    pub named_classes: IndexMap<SmolStr, GlyphClass>,
    features: IndexMap<SmolStr, Vec<LookupReferenceStatement>>,
}

impl<'a> UncompileContext<'a> {
    fn new(font: &'a skrifa::FontRef) -> Result<Self, ReadError> {
        let glyph_names = GlyphNames::new(font);
        Ok(Self {
            lookups: IndexMap::new(),
            lookup_map: HashMap::new(),
            symbols: IndexMap::new(),
            gpos: font.gpos().ok(),
            gsub: font.gsub().ok(),
            gdef: font.gdef().ok(),
            language_systems: IndexMap::new(),
            unnamed_anchors: IndexMap::new(),
            anchors: IndexMap::new(),
            glyph_names,
            mark_classes: IndexMap::new(),
            named_classes: IndexMap::new(),
            features: IndexMap::new(),
        })
    }

    fn register_anchor(&mut self, glyphname: &SmolStr, anchor: &Anchor, anchor_name: Option<&str>) {
        if let Some(anchor_name) = anchor_name {
            self.anchors
                .entry(anchor_name.to_string().into())
                .or_default()
                .insert(glyphname.clone(), anchor.clone());
        } else {
            self.unnamed_anchors
                .entry(glyphname.clone())
                .or_default()
                .push(anchor.clone());
        }
    }

    fn register_mark_classes(
        &mut self,
        mark_classes: IndexMap<u16, Vec<(GlyphContainer, Anchor)>>,
    ) -> Vec<SmolStr> {
        let mut class_names = vec![];
        for (_class_id, anchors) in mark_classes.iter() {
            let class_name = format!("mark_class_{}", self.mark_classes.len());
            let definitions = anchors
                .iter()
                .map(|(members, anchor)| {
                    MarkClassDefinition::new(
                        MarkClass::new(&class_name),
                        anchor.clone(),
                        members.clone(),
                    )
                })
                .collect();
            self.mark_classes
                .insert(class_name.clone().into(), definitions);
            class_names.push(class_name.into());
        }
        class_names
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
    fn resolve_coverage_to_class(&mut self, coverage: &CoverageTable) -> GlyphContainer {
        let glyphs = self.resolve_coverage(coverage);
        if glyphs.len() == 1 {
            return glyphs.into_iter().next().unwrap();
        }
        let glyphclass = GlyphClass::new(glyphs.clone(), 0..0);

        if glyphs.len() >= PROMOTE_TO_NAMED_CLASS_THRESHOLD {
            // Have we seen this exact set of glyphs before (in order)? If so, reuse the same class. Otherwise, make a new one.
            if let Some(_class_name) = self
                .named_classes
                .iter()
                .find(|(_, class)| class.glyphs == glyphclass.glyphs)
                .map(|(name, _)| name)
            {
                GlyphContainer::GlyphClass(GlyphClass::new(glyphclass.glyphs.clone(), 0..0))
            } else {
                let class_name = format!("class_{}", self.named_classes.len());
                self.named_classes
                    .insert(class_name.clone().into(), glyphclass.clone());
                GlyphContainer::GlyphClass(GlyphClass::new(glyphclass.glyphs.clone(), 0..0))
            }
        } else {
            GlyphContainer::GlyphClass(glyphclass)
        }
    }

    fn resolve_classes(&self, class_def: ClassDef) -> HashMap<u16, Vec<GlyphContainer>> {
        let mut classes: HashMap<u16, Vec<GlyphContainer>> = HashMap::new();
        let mut used_glyphs = HashSet::new();
        match class_def {
            ClassDef::Format1(class_def1) => {
                for (class_id, glyph_id) in class_def1.class_value_array().iter().enumerate() {
                    used_glyphs.insert(glyph_id.get());
                    // We +1 to leave room for class 0, which is the "unclassified" class that we will fill in later with any glyphs not mentioned in the class def
                    classes.entry((class_id + 1) as u16).or_default().push(
                        GlyphContainer::GlyphName(self.get_name(GlyphId16::new(glyph_id.get()))),
                    );
                }
            }
            ClassDef::Format2(class_def2) => {
                for (class_id, range) in class_def2.class_range_records().iter().enumerate() {
                    for gid in range.start_glyph_id().to_u16()..=range.end_glyph_id().to_u16() {
                        used_glyphs.insert(gid);
                        classes.entry((class_id + 1) as u16).or_default().push(
                            GlyphContainer::GlyphName(self.get_name(GlyphId16::new(gid))),
                        );
                    }
                }
            }
        }
        // Now fill class 0 with unused glyphs from font
        let full_font: HashSet<_> = (0..self.glyph_names.num_glyphs() as u16).collect();
        classes.insert(
            0,
            full_font
                .difference(&used_glyphs)
                .map(|gid| GlyphContainer::GlyphName(self.get_name(GlyphId16::new(*gid))))
                .collect(),
        );
        classes
    }
    fn create_next_lookup_block<T: SubOrPos>(
        &mut self,
        prefix: &str,
        index: u16,
        phase: T,
    ) -> LookupBlock {
        let symbol_index = self.symbols.entry(prefix.into()).or_insert(1);
        let i = *symbol_index;
        let name = SmolStr::new(format!("{}_{}", prefix, i));
        *symbol_index += 1;
        self.lookup_map
            .insert((phase.to_string(), index), name.clone());
        LookupBlock::new(name.clone(), vec![], false, 0..0)
    }

    fn get_lookup_name<T: SubOrPos>(&self, lookup_list_index: u16, phase: T) -> SmolStr {
        self.lookup_map
            .get(&(phase.to_string(), lookup_list_index))
            .cloned()
            .unwrap_or_else(|| format!("{}_lookup_{}", phase, lookup_list_index).into())
    }

    fn uncompile_gdef(&mut self) -> Result<Vec<ToplevelItem>, ReadError> {
        let mut items = vec![];
        let mut base_glyphs = vec![];
        let mut mark_glyphs = vec![];
        let mut ligature_glyphs = vec![];
        let mut component_glyphs = vec![];
        let make_class =
            |v: Vec<GlyphContainer>| Some(GlyphContainer::GlyphClass(GlyphClass::new(v, 0..0)));
        if let Some(gdef) = &self.gdef {
            // Uncompile glyph categories
            if let Some(Ok(glyph_class_def)) = gdef.glyph_class_def() {
                for (gid, class) in glyph_class_def.iter() {
                    let name = self.get_name(gid);
                    match class {
                        1 => base_glyphs.push(GlyphContainer::GlyphName(name)),
                        2 => ligature_glyphs.push(GlyphContainer::GlyphName(name)),
                        3 => mark_glyphs.push(GlyphContainer::GlyphName(name)),
                        4 => component_glyphs.push(GlyphContainer::GlyphName(name)),
                        _ => {}
                    }
                }
                items.push(ToplevelItem::GdefClassDef(GlyphClassDefStatement::new(
                    make_class(base_glyphs),
                    make_class(ligature_glyphs),
                    make_class(mark_glyphs),
                    make_class(component_glyphs),
                    0..0,
                )));
            }
        }
        Ok(items)
    }

    fn uncompile_feature_table(&mut self) -> Result<(), ReadError> {
        if let Some(gsub) = &self.gsub {
            for feature_record in gsub.feature_list()?.feature_records() {
                let feature_tag = feature_record.feature_tag();
                let feature = feature_record.feature(gsub.feature_list()?.offset_data())?;
                let lookup_indices = feature.lookup_list_indices();
                self.features.insert(
                    feature_tag.to_string().into(),
                    lookup_indices
                        .iter()
                        .map(|i| {
                            LookupReferenceStatement::new(
                                self.get_lookup_name(i.get(), Subst).into(),
                                0..0,
                            )
                        })
                        .collect(),
                );
            }
        }

        if let Some(gpos) = &self.gpos {
            for feature_record in gpos.feature_list()?.feature_records() {
                let feature_tag = feature_record.feature_tag();
                let feature = feature_record.feature(gpos.feature_list()?.offset_data())?;
                let lookup_indices = feature.lookup_list_indices();
                self.features.insert(
                    feature_tag.to_string().into(),
                    lookup_indices
                        .iter()
                        .map(|i| {
                            LookupReferenceStatement::new(
                                self.get_lookup_name(i.get(), Pos).into(),
                                0..0,
                            )
                        })
                        .collect(),
                );
            }
        }

        Ok(())
    }
}

/// Uncompile a TTF font into a fea file.
///
/// If do_gdef is true, also uncompile the GDEF table and include it in the output.
/// Returns a [fea_rs_ast::FeatureFile] representing the uncompiled font, or a ReadError if something went wrong during reading.
pub fn uncompile(
    font: &skrifa::FontRef,
    do_gdef: bool,
) -> Result<fea_rs_ast::FeatureFile, ReadError> {
    let mut context = UncompileContext::new(font)?;
    context.gather_language_systems()?;
    let mut ff = fea_rs_ast::FeatureFile::new(vec![]);
    ff.statements.extend(context.dump_language_systems());

    if do_gdef {
        ff.statements.extend(context.uncompile_gdef()?);
    }

    context.uncompile_gsub_lookups()?;
    context.uncompile_gpos_lookups()?;
    // Add mark classes to the feature file
    for definitions in context.mark_classes.values() {
        for definition in definitions {
            ff.statements
                .push(ToplevelItem::MarkClassDefinition(definition.clone()));
        }
    }
    // Add named class definitions
    for (name, contents) in context.named_classes.iter() {
        ff.statements.push(ToplevelItem::GlyphClassDefinition(
            GlyphClassDefinition::new(name.to_string(), contents.clone(), 0..0),
        ));
    }
    // Add all lookups to the feature file
    for lookup in context.lookups.values() {
        ff.statements.push(ToplevelItem::Lookup(lookup.clone()));
    }
    context.uncompile_feature_table()?;
    // Add all feature references to the feature file
    for (feature_name, lookup_refs) in context.features.iter() {
        ff.statements
            .push(ToplevelItem::Feature(fea_rs_ast::FeatureBlock::new(
                feature_name.clone(),
                lookup_refs
                    .iter()
                    .map(|lr| Statement::LookupReference(lr.clone()))
                    .collect(),
                false,
                0..0,
            )));
    }

    Ok(ff)
}

/// Uncompile a TTF font from a byte slice into a fea file. See uncompile() for details.
pub fn uncompile_bytes(
    font_data: &[u8],
    do_gdef: bool,
) -> Result<fea_rs_ast::FeatureFile, ReadError> {
    let fontref = skrifa::FontRef::new(font_data)?;
    uncompile(&fontref, do_gdef)
}

/// Uncompile a TTF font to a context object.
///
/// This partially decompiles the font, giving you the component parts so that you can
/// put them where you want them. Useful for font editors and other tools that want the
/// data but don't want to go all the way to a fea file.
pub fn uncompile_context<'a>(
    font: &'a skrifa::FontRef,
    do_gdef: bool,
) -> Result<UncompileContext<'a>, ReadError> {
    let mut context = UncompileContext::new(font)?;
    context.gather_language_systems()?;
    if do_gdef {
        context.uncompile_gdef()?;
    }

    context.uncompile_gsub_lookups()?;
    context.uncompile_gpos_lookups()?;
    context.uncompile_feature_table()?;
    Ok(context)
}

#[cfg(test)]
mod tests {
    use fea_rs_ast::AsFea;

    use super::*;
    #[test]
    fn test_uncompile_static() {
        let data = std::fs::read("resources/test.ttf").unwrap();
        let fontref = skrifa::FontRef::new(&data).unwrap();
        let ff = uncompile(&fontref, true).unwrap();
        assert_eq!(
            ff.as_fea(""),
            "GlyphClassDef [A], [], [grave acute dotbelowcomb], [];\nmarkClass grave <anchor 200 150> @mark_class_0;\nmarkClass acute <anchor 350 0> @mark_class_0;\nmarkClass dotbelowcomb <anchor 200 -200> @mark_class_1;\nlookup gsub_single_1 {\n    sub a by b;\n} gsub_single_1;\nlookup gsub_multiple_1 {\n    sub a by b c;\n} gsub_multiple_1;\nlookup gsub_alternate_1 {\n    sub a from [b c d e f];\n} gsub_alternate_1;\nlookup gsub_ligature_1 {\n    sub b c by a;\n} gsub_ligature_1;\nlookup gsub_contextual_1 {\n    sub [one a]' lookup gsub_single_1 b' [two c]' lookup gsub_multiple_1;\n} gsub_contextual_1;\nlookup gsub_chain_contextual_1 {\n    sub one two three a' lookup gsub_single_1 b' c' lookup gsub_multiple_1 x y z;\n} gsub_chain_contextual_1;\nlookup gpos_mark_to_base_1 {\n    pos base A\n        <anchor 150 100> mark @mark_class_0\n        <anchor -200 -200> mark @mark_class_1;\n} gpos_mark_to_base_1;\n"
        );
    }
}
