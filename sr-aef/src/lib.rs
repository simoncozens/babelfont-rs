//! # sr-aef
//!
//! `sr-aef` "uncompiles" an OpenType/TrueType font back into Adobe Feature File
//! (`fea`) source code. It reads the OpenType layout tables (GSUB, GPOS and,
//! optionally, GDEF) out of a compiled binary font and reconstructs an equivalent
//! feature file using the [`fea_rs_ast`] AST.
//!
//! Because binary fonts lose the original structure of their layout code (lookup
//! splitting, class naming, comments, ...), the output is a faithful *functional*
//! reconstruction rather than a byte-for-byte copy of the original source.
//!
//! The main entry points are [`uncompile`], [`uncompile_bytes`] and
//! [`uncompile_context`]:
//!
//! ```no_run
//! use sr_aef::{fea_rs_ast::AsFea, uncompile_bytes};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let data = std::fs::read("MyFont.ttf")?;
//!     // `true` also uncompiles the GDEF table.
//!     let feature_file = uncompile_bytes(&data, true)?;
//!     println!("{}", feature_file.as_fea(""));
//!     Ok(())
//! }
//! ```
//!
//! The crate also powers the TrueType round-trip in
//! [babelfont](https://crates.io/crates/babelfont).
use std::collections::{HashMap, HashSet};

/// A handle to the version of fea-rs-ast that sr-aef is using.
///
/// The return value of uncompile() will be a [fea_rs_ast::FeatureFile]; you will probably want to call `.as_fea()` on it.
pub use fea_rs_ast;
use fea_rs_ast::{
    Anchor, AsFea, AttachStatement, Comment, GdefStatement, GlyphClass, GlyphClassDefStatement,
    GlyphClassDefinition, GlyphContainer, GlyphName, LanguageStatement, LanguageSystemStatement,
    LigatureCaretByIndexStatement, LigatureCaretByPosStatement, LookupBlock, LookupFlagStatement,
    LookupReferenceStatement, MarkClass, MarkClassDefinition, Pos, ScriptStatement, Statement,
    SubOrPos, Subst, Table, ToplevelItem,
};
use indexmap::{IndexMap, IndexSet};
/// A handle to the version of Skrifa that sr-eaf is using. Pass a skrifa::FontRef to uncompile()
pub use skrifa;
use skrifa::{
    GlyphId, GlyphId16, GlyphNames, Tag,
    metrics::GlyphMetrics,
    prelude::{LocationRef, Size},
    raw::{
        ReadError, TableProvider,
        tables::{
            gdef::{CaretValue, Gdef},
            gpos::Gpos,
            gsub::{ClassDef, Gsub},
            layout::{CoverageTable, LookupFlag, ScriptList},
        },
    },
};
use smol_str::SmolStr;

mod contextual;
mod gpos;
mod gsub;
#[cfg(feature = "cli")]
mod serialize;
mod variations;

pub(crate) type SimpleUserLocation = IndexMap<SmolStr, i16>; // as used by fea-rs-ast metrics

const PROMOTE_TO_NAMED_CLASS_THRESHOLD: usize = 5;

/// The context object that holds all the information we need when uncompiling a font.
#[cfg_attr(feature = "cli", derive(serde::Serialize))]
pub struct UncompileContext<'a> {
    /// All the lookups we have uncompiled so far, keyed by their name.
    pub lookups: IndexMap<SmolStr, LookupBlock>,
    /// A mapping from ("sub"|"pos", lookup_list_index) to the name of the lookup block we created for it. This is used to generate the correct lookup names in contextual lookups.
    #[cfg_attr(
        feature = "cli",
        serde(serialize_with = "crate::serialize::serialize_lookup_map")
    )]
    pub lookup_map: HashMap<(String, u16), SmolStr>,
    /// A mapping from script tags to the language system tags that are present in the font.
    #[cfg_attr(
        feature = "cli",
        serde(serialize_with = "crate::serialize::serialize_language_systems")
    )]
    pub language_systems: IndexMap<Tag, IndexSet<Tag>>,
    /// Anchors on a glyph which we haven't worked out what they should be called.
    /// class -> glyphname -> anchor
    pub unnamed_anchors: IndexMap<SmolStr, Vec<Anchor>>,
    /// Anchors on a glyph which we have worked out what they should be called.
    pub anchors: IndexMap<SmolStr, IndexMap<SmolStr, Anchor>>,
    /// Mark classes, indexed by class name.
    pub mark_classes: IndexMap<SmolStr, Vec<MarkClassDefinition>>,
    /// Named glyph classes, indexed by class name.
    #[cfg_attr(
        feature = "cli",
        serde(serialize_with = "crate::serialize::serialize_named_classes")
    )]
    pub named_classes: IndexMap<SmolStr, GlyphClass>,
    /// Features, indexed by feature name.
    #[cfg_attr(
        feature = "cli",
        serde(serialize_with = "crate::serialize::serialize_features")
    )]
    pub features: IndexMap<SmolStr, Vec<Statement>>,
    #[cfg_attr(feature = "cli", serde(skip))]
    symbols: IndexMap<SmolStr, usize>,
    #[cfg_attr(feature = "cli", serde(skip))]
    gpos: Option<Gpos<'a>>,
    #[cfg_attr(feature = "cli", serde(skip))]
    gsub: Option<Gsub<'a>>,
    #[cfg_attr(feature = "cli", serde(skip))]
    gdef: Option<Gdef<'a>>,
    #[cfg_attr(feature = "cli", serde(skip))]
    glyph_metrics: GlyphMetrics<'a>,
    #[cfg_attr(feature = "cli", serde(skip))]
    glyph_id_to_name: HashMap<GlyphId, SmolStr>,
    #[cfg_attr(feature = "cli", serde(skip))]
    glyph_name_to_id: HashMap<SmolStr, GlyphId>,
    #[cfg_attr(feature = "cli", serde(skip))]
    axis_tags: Vec<Tag>,
    #[cfg_attr(feature = "cli", serde(skip))]
    axes: Option<fontdrasil::types::Axes>,
    num_glyphs: u16,
}

impl<'a> UncompileContext<'a> {
    fn new(font: &'a skrifa::FontRef) -> Result<Self, ReadError> {
        let glyph_names = GlyphNames::new(font);
        let default = LocationRef::default();
        let glyph_metrics = GlyphMetrics::new(font, Size::unscaled(), default);
        let glyph_id_to_name: HashMap<GlyphId, SmolStr> = (0..glyph_names.num_glyphs())
            .map(GlyphId::new)
            .map(|gid| {
                (
                    gid,
                    SmolStr::new(
                        glyph_names
                            .get(gid)
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("gid{:04}", gid)),
                    ),
                )
            })
            .collect();
        let glyph_name_to_id = glyph_id_to_name
            .iter()
            .map(|(gid, name): (&GlyphId, &SmolStr)| (name.clone(), *gid))
            .collect();
        let mut slf = Self {
            lookups: IndexMap::new(),
            lookup_map: HashMap::new(),
            symbols: IndexMap::new(),
            gpos: font.gpos().ok(),
            gsub: font.gsub().ok(),
            gdef: font.gdef().ok(),
            language_systems: IndexMap::new(),
            unnamed_anchors: IndexMap::new(),
            anchors: IndexMap::new(),
            mark_classes: IndexMap::new(),
            named_classes: IndexMap::new(),
            features: IndexMap::new(),
            glyph_metrics,
            glyph_id_to_name,
            glyph_name_to_id,
            axis_tags: font
                .fvar()
                .ok()
                .and_then(|fvar| fvar.axes().ok())
                .map(|axes| axes.iter().map(|axis| axis.axis_tag()).collect())
                .unwrap_or_default(),
            axes: variations::fontdrasil_axes(font)?,
            num_glyphs: glyph_names.num_glyphs() as u16,
        };
        slf.gather_language_systems()?;
        slf.uncompile_gsub_lookups()?;
        slf.uncompile_gpos_lookups()?;
        slf.uncompile_feature_table()?;
        slf.order_lookups();
        Ok(slf)
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

    fn register_mark_class(
        &mut self,
        anchors: Vec<(GlyphContainer, Anchor)>,
        class_name: &SmolStr,
    ) {
        let definitions = anchors
            .iter()
            .map(|(members, anchor)| {
                MarkClassDefinition::new(
                    MarkClass::new(class_name),
                    anchor.clone(),
                    members.clone(),
                )
            })
            .collect();
        self.mark_classes.insert(class_name.clone(), definitions);
    }

    fn get_name(&self, id: GlyphId16) -> GlyphName {
        let str: SmolStr = self
            .glyph_id_to_name
            .get(&GlyphId::new(id.to_u32()))
            .cloned()
            .unwrap_or_else(|| format!("gid{:04}", id.to_u32()).into());
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
        if let Some(index) = systems.get_index_of(&Tag::new(b"DFLT")) {
            systems.move_index(index, 0);
            let languages = &mut systems[0];
            if let Some(index) = languages.get_index_of(&Tag::new(b"dflt")) {
                languages.move_index(index, 0);
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
    fn resolve_coverage_to_class(
        &mut self,
        coverage: &CoverageTable,
        collapse_single: bool,
    ) -> GlyphContainer {
        let glyphs = self.resolve_coverage(coverage);
        if collapse_single && glyphs.len() == 1 {
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

    fn resolve_classes(&self, class_def: Option<&ClassDef>) -> HashMap<u16, Vec<GlyphContainer>> {
        let mut classes: HashMap<u16, Vec<GlyphContainer>> = HashMap::new();
        let mut used_glyphs = HashSet::new();
        for (glyph_id, class_id) in class_def.into_iter().flat_map(|class_def| class_def.iter()) {
            used_glyphs.insert(glyph_id.to_u16());
            classes
                .entry(class_id)
                .or_default()
                .push(GlyphContainer::GlyphName(self.get_name(glyph_id)));
        }

        // Class 0 is all glyphs that are not explicitly assigned by the ClassDef.
        for gid in 0..self.num_glyphs {
            if !used_glyphs.contains(&gid) {
                classes
                    .entry(0)
                    .or_default()
                    .push(GlyphContainer::GlyphName(
                        self.get_name(GlyphId16::new(gid)),
                    ));
            }
        }
        classes
    }

    pub(crate) fn gensym(&mut self, prefix: &str) -> SmolStr {
        let symbol_index = self.symbols.entry(prefix.into()).or_insert(1);
        let name = SmolStr::new(format!("{}_{}", prefix, symbol_index));
        *symbol_index += 1;
        name
    }

    fn assign_lookup_name<T: SubOrPos>(&mut self, prefix: &str, index: u16, phase: T) {
        let name = self.gensym(prefix);
        self.lookup_map.insert((phase.to_string(), index), name);
    }

    fn create_lookup_block<T: SubOrPos>(&self, index: u16, phase: T) -> LookupBlock {
        LookupBlock::new(self.get_lookup_name(index, phase), vec![], false, 0..0)
    }

    fn get_lookup_name<T: SubOrPos>(&self, lookup_list_index: u16, phase: T) -> SmolStr {
        self.lookup_map
            .get(&(phase.to_string(), lookup_list_index))
            .cloned()
            .unwrap_or_else(|| format!("{}_lookup_{}", phase, lookup_list_index).into())
    }

    /// Feature files require lookups to be defined before use. Reorder them while
    /// keeping the original order where possible.
    fn order_lookups(&mut self) {
        let used_by_features: HashSet<SmolStr> = self
            .features
            .values()
            .flatten()
            .filter_map(|statement| match statement {
                Statement::LookupReference(reference) => Some(SmolStr::new(&reference.lookup_name)),
                _ => None,
            })
            .collect();
        let calls: HashMap<SmolStr, Vec<SmolStr>> = self
            .lookups
            .iter()
            .map(|(name, lookup)| (name.clone(), called_lookups(lookup)))
            .collect();
        let contextual_only: HashSet<SmolStr> = calls
            .values()
            .flatten()
            .filter(|name| !used_by_features.contains(*name))
            .cloned()
            .collect();
        let feature_dependencies: HashMap<SmolStr, Vec<SmolStr>> = calls
            .keys()
            .map(|name| {
                (
                    name.clone(),
                    feature_dependencies(name, &calls, &contextual_only),
                )
            })
            .collect();
        let count_pending_dependencies =
            |name: &SmolStr, pending: &IndexMap<SmolStr, LookupBlock>| {
                feature_dependencies[name]
                    .iter()
                    .filter(|dependency| pending.contains_key(*dependency))
                    .count()
            };

        let mut pending = std::mem::take(&mut self.lookups);
        let mut deferred: Vec<SmolStr> = vec![];
        let names: Vec<SmolStr> = pending.keys().cloned().collect();
        for name in names {
            if !pending.contains_key(&name) {
                continue;
            }
            let pending_dependencies = count_pending_dependencies(&name, &pending);
            if contextual_only.contains(&name) {
                if pending_dependencies == 0 {
                    self.place_lookup(&name, &mut pending, &calls);
                }
                continue;
            }
            if pending_dependencies > 1 {
                deferred.push(name);
                continue;
            }
            self.place_lookup(&name, &mut pending, &calls);
            while let Some(index) = deferred
                .iter()
                .position(|name| count_pending_dependencies(name, &pending) == 0)
            {
                let name = deferred.remove(index);
                self.place_lookup(&name, &mut pending, &calls);
            }
        }
        let remaining: Vec<SmolStr> = pending.keys().cloned().collect();
        for name in deferred.into_iter().chain(remaining) {
            self.place_lookup(&name, &mut pending, &calls);
        }
    }

    fn place_lookup(
        &mut self,
        name: &SmolStr,
        pending: &mut IndexMap<SmolStr, LookupBlock>,
        calls: &HashMap<SmolStr, Vec<SmolStr>>,
    ) {
        let Some(lookup) = pending.shift_remove(name) else {
            return;
        };
        for dependency in calls.get(name).into_iter().flatten() {
            self.place_lookup(dependency, pending, calls);
        }
        self.lookups.insert(name.clone(), lookup);
    }

    fn uncompile_gdef(&mut self) -> Result<Vec<ToplevelItem>, ReadError> {
        let mut statements = vec![];
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
                statements.push(GdefStatement::GlyphClassDef(GlyphClassDefStatement::new(
                    make_class(base_glyphs),
                    make_class(ligature_glyphs),
                    make_class(mark_glyphs),
                    make_class(component_glyphs),
                    0..0,
                )));
            }
            if let Some(Ok(attach_list)) = gdef.attach_list() {
                let coverage = attach_list.coverage()?;
                for (gid, attach_point) in coverage.iter().zip(attach_list.attach_points().iter()) {
                    let point_indices: Vec<usize> = attach_point?
                        .point_indices()
                        .iter()
                        .map(|index| index.get() as usize)
                        .collect();
                    if !point_indices.is_empty() {
                        statements.push(GdefStatement::Attach(AttachStatement::new(
                            GlyphContainer::GlyphName(self.get_name(gid)),
                            point_indices,
                            0..0,
                        )));
                    }
                }
            }
            if let Some(Ok(lig_caret_list)) = gdef.lig_caret_list() {
                let coverage = lig_caret_list.coverage()?;
                for (gid, lig_glyph) in coverage.iter().zip(lig_caret_list.lig_glyphs().iter()) {
                    let mut positions = vec![];
                    let mut point_indices = vec![];
                    for caret in lig_glyph?.caret_values().iter() {
                        match caret? {
                            CaretValue::Format1(caret) => positions.push(caret.coordinate()),
                            CaretValue::Format2(caret) => {
                                point_indices.push(caret.caret_value_point_index() as usize)
                            }
                            CaretValue::Format3(caret) => positions.push(caret.coordinate()),
                        }
                    }
                    let glyph = GlyphContainer::GlyphName(self.get_name(gid));
                    if !positions.is_empty() {
                        statements.push(GdefStatement::LigatureCaretByPos(
                            LigatureCaretByPosStatement::new(glyph.clone(), positions, 0..0),
                        ));
                    }
                    if !point_indices.is_empty() {
                        statements.push(GdefStatement::LigatureCaretByIndex(
                            LigatureCaretByIndexStatement::new(glyph, point_indices, 0..0),
                        ));
                    }
                }
            }
        }
        if statements.is_empty() {
            return Ok(vec![]);
        }
        Ok(vec![ToplevelItem::Gdef(Table { statements })])
    }

    fn uncompile_feature_table(&mut self) -> Result<(), ReadError> {
        let mut registered_features = vec![];
        let mut unregistered_features = vec![];
        if let Some(feature_list) = self.gsub.as_ref().and_then(|gsub| gsub.feature_list().ok()) {
            let systems =
                feature_language_systems(self.gsub.as_ref().and_then(|g| g.script_list().ok()))?;
            for (index, feature_record) in feature_list.feature_records().iter().enumerate() {
                let feature_tag = feature_record.feature_tag();
                let feature = feature_record.feature(feature_list.offset_data())?;
                let lookup_indices = feature.lookup_list_indices();
                let lookups = lookup_indices
                    .iter()
                    .map(|i| lookup_reference(&self.get_lookup_name(i.get(), Subst)))
                    .collect();
                if let Some(language_systems) = systems.get(&(index as u16)) {
                    registered_features.push((feature_tag, language_systems.clone(), lookups));
                } else {
                    unregistered_features.push((feature_tag, lookups));
                }
            }
        }

        if let Some(feature_list) = self.gpos.as_ref().and_then(|gpos| gpos.feature_list().ok()) {
            let systems =
                feature_language_systems(self.gpos.as_ref().and_then(|g| g.script_list().ok()))?;
            for (index, feature_record) in feature_list.feature_records().iter().enumerate() {
                let feature_tag = feature_record.feature_tag();
                let feature = feature_record.feature(feature_list.offset_data())?;
                let lookup_indices = feature.lookup_list_indices();
                let lookups = lookup_indices
                    .iter()
                    .map(|i| lookup_reference(&self.get_lookup_name(i.get(), Pos)))
                    .collect();
                if let Some(language_systems) = systems.get(&(index as u16)) {
                    registered_features.push((feature_tag, language_systems.clone(), lookups));
                } else {
                    unregistered_features.push((feature_tag, lookups));
                }
            }
        }
        self.add_registered_features(registered_features);

        let referenced_lookups: Vec<Statement> = self
            .features
            .values()
            .flatten()
            .cloned()
            .chain(
                self.lookups
                    .values()
                    .flat_map(called_lookups)
                    .map(|name| lookup_reference(&name)),
            )
            .collect();
        for (feature_tag, lookups) in unregistered_features {
            let lookups = lookups
                .into_iter()
                .filter(|lookup| !referenced_lookups.contains(lookup))
                .map(comment_out)
                .collect();
            self.add_feature_lookups(feature_tag, lookups);
        }

        Ok(())
    }

    fn add_registered_features(
        &mut self,
        features: Vec<(Tag, Vec<((Tag, Tag), bool)>, Vec<Statement>)>,
    ) {
        let mut registrations: IndexMap<Tag, IndexMap<(Tag, Tag), (bool, Vec<Statement>)>> =
            IndexMap::new();
        for (feature_tag, language_systems, lookups) in features {
            for (language_system, required) in language_systems {
                let (is_required, registered) = registrations
                    .entry(feature_tag)
                    .or_default()
                    .entry(language_system)
                    .or_default();
                *is_required |= required;
                for lookup in &lookups {
                    if !registered.contains(lookup) {
                        registered.push(lookup.clone());
                    }
                }
            }
        }

        let dflt = Tag::new(b"dflt");
        let system_count: usize = self.language_systems.values().map(|l| l.len()).sum();
        for (feature_tag, systems) in registrations {
            let (_, first) = &systems[0];
            let everywhere = systems.len() == system_count
                && systems.values().all(|(required, lookups)| {
                    !required
                        && lookups.len() == first.len()
                        && lookups.iter().all(|l| first.contains(l))
                });
            if everywhere || [Tag::new(b"aalt"), Tag::new(b"size")].contains(&feature_tag) {
                for (_, lookups) in systems.into_values() {
                    self.add_feature_lookups(feature_tag, lookups);
                }
                continue;
            }
            let mut statements = vec![];
            for (script_tag, languages) in &self.language_systems {
                if !languages
                    .iter()
                    .any(|language| systems.contains_key(&(*script_tag, *language)))
                {
                    continue;
                }
                statements.push(Statement::Script(ScriptStatement::new(
                    script_tag.to_string().trim_end().into(),
                )));
                let (default_required, default_lookups) = systems
                    .get(&(*script_tag, dflt))
                    .cloned()
                    .unwrap_or_default();
                if default_required {
                    statements.push(Statement::Language(LanguageStatement::new(
                        dflt.to_string(),
                        true,
                        true,
                    )));
                }
                statements.extend(default_lookups.iter().cloned());
                for language in languages.iter().filter(|language| **language != dflt) {
                    let Some((required, lookups)) = systems.get(&(*script_tag, *language)) else {
                        continue;
                    };
                    let include_dflt = default_lookups.iter().all(|l| lookups.contains(l));
                    statements.push(Statement::Language(LanguageStatement::new(
                        language.to_string().trim_end().into(),
                        include_dflt,
                        *required,
                    )));
                    statements.extend(
                        lookups
                            .iter()
                            .filter(|l| !include_dflt || !default_lookups.contains(l))
                            .cloned(),
                    );
                }
            }
            self.features
                .insert(feature_tag.to_string().into(), statements);
        }
    }

    fn add_feature_lookups(&mut self, feature_tag: Tag, lookups: Vec<Statement>) {
        let statements = self
            .features
            .entry(feature_tag.to_string().into())
            .or_default();
        for lookup in lookups {
            if !statements.contains(&lookup) {
                statements.push(lookup);
            }
        }
    }

    fn add_lookup_flags(
        &mut self,
        lookupblock: &mut LookupBlock,
        flags: LookupFlag,
        mark_filtering_set: Option<u16>,
    ) {
        if flags == LookupFlag::empty() {
            return;
        }
        let mark_glyph_sets = self.gdef.as_ref().and_then(|x| {
            let mark_glyph_sets = x.mark_glyph_sets_def();
            if let Some(Ok(mark_glyph_sets)) = mark_glyph_sets {
                Some(mark_glyph_sets)
            } else {
                None
            }
        });
        let mark_attachment_classes = self.gdef.as_ref().and_then(|x| {
            let mark_attachment_classes = x.mark_attach_class_def();
            if let Some(Ok(mark_attachment_classes)) = mark_attachment_classes {
                Some(mark_attachment_classes)
            } else {
                None
            }
        });
        let set = mark_filtering_set.and_then(|set| {
            mark_glyph_sets
                .and_then(|mgss| mgss.coverages().get(set as usize).ok())
                .map(|coverage| self.resolve_coverage_to_class(&coverage, false))
        });
        let mark_attachment_class = flags.mark_attachment_class().map(|class| {
            let classes = mark_attachment_classes
                .and_then(|mac| self.resolve_classes(Some(&mac)).get(&class).cloned())
                .unwrap_or_default();
            GlyphContainer::GlyphClass(GlyphClass::new(classes, 0..0))
        });

        lookupblock.statements.insert(
            0,
            Statement::LookupFlag(LookupFlagStatement::new(
                flags.to_bits(),
                mark_attachment_class,
                set,
                0..0,
            )),
        );
    }
}

fn lookup_reference(name: &SmolStr) -> Statement {
    Statement::LookupReference(LookupReferenceStatement::new(name.to_string(), 0..0))
}

fn comment_out(statement: Statement) -> Statement {
    Statement::Comment(Comment::new(format!("# {}", statement.as_fea(""))))
}

fn feature_language_systems(
    script_list: Option<ScriptList>,
) -> Result<HashMap<u16, Vec<((Tag, Tag), bool)>>, ReadError> {
    let mut systems: HashMap<u16, Vec<((Tag, Tag), bool)>> = HashMap::new();
    let Some(script_list) = script_list else {
        return Ok(systems);
    };
    for script_record in script_list.script_records() {
        let script = script_record.script(script_list.offset_data())?;
        let mut language_systems = vec![];
        if let Some(lang_sys) = script.default_lang_sys() {
            language_systems.push((Tag::new(b"dflt"), lang_sys?));
        }
        for lang_sys_record in script.lang_sys_records() {
            language_systems.push((
                lang_sys_record.lang_sys_tag(),
                lang_sys_record.lang_sys(script.offset_data())?,
            ));
        }
        for (lang_sys_tag, lang_sys) in language_systems {
            let required_feature_index = lang_sys.required_feature_index();
            let indices = lang_sys.feature_indices().iter().map(|index| index.get());
            for index in
                indices.chain((required_feature_index != 0xFFFF).then_some(required_feature_index))
            {
                systems.entry(index).or_default().push((
                    (script_record.script_tag(), lang_sys_tag),
                    index == required_feature_index,
                ));
            }
        }
    }
    Ok(systems)
}

fn called_lookups(lookup: &LookupBlock) -> Vec<SmolStr> {
    lookup
        .statements
        .iter()
        .flat_map(|statement| match statement {
            Statement::ChainedContextSubst(statement) => statement.lookups.concat(),
            Statement::ChainedContextPos(statement) => statement.lookups.concat(),
            _ => vec![],
        })
        .collect()
}

/// Find dependencies used by features, following calls through lookups unused by features.
fn feature_dependencies(
    name: &SmolStr,
    calls: &HashMap<SmolStr, Vec<SmolStr>>,
    contextual_only: &HashSet<SmolStr>,
) -> Vec<SmolStr> {
    let mut dependencies = vec![];
    let mut seen = HashSet::new();
    let mut stack: Vec<&SmolStr> = calls.get(name).into_iter().flatten().collect();
    while let Some(dependency) = stack.pop() {
        if !seen.insert(dependency) {
            continue;
        }
        if contextual_only.contains(dependency) {
            stack.extend(calls.get(dependency).into_iter().flatten());
        } else {
            dependencies.push(dependency.clone());
        }
    }
    dependencies
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

    let mut ff = fea_rs_ast::FeatureFile::new(vec![]);
    ff.statements.extend(context.dump_language_systems());

    if do_gdef {
        ff.statements.extend(context.uncompile_gdef()?);
    }

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
    // Add all feature references to the feature file
    for (feature_name, lookup_refs) in context.features.iter() {
        ff.statements
            .push(ToplevelItem::Feature(fea_rs_ast::FeatureBlock::new(
                feature_name.clone(),
                lookup_refs.clone(),
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
pub fn uncompile_context<'a>(font: &'a skrifa::FontRef) -> Result<UncompileContext<'a>, ReadError> {
    UncompileContext::new(font)
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
            "\
languagesystem DFLT dflt;
table GDEF {
GlyphClassDef [A], [], [grave acute dotbelowcomb], [];
} GDEF;

markClass grave <anchor 200 150> @bottomleft;
markClass acute <anchor 350 0> @bottomleft;
markClass dotbelowcomb <anchor 200 -200> @mark_class_1_1;
lookup gsub_single_1 {
    sub a by b;
} gsub_single_1;
lookup gsub_multiple_1 {
    sub a by b c;
} gsub_multiple_1;
lookup gsub_alternate_1 {
    sub a from [b c d e f];
} gsub_alternate_1;
lookup gsub_ligature_1 {
    sub b c by a;
} gsub_ligature_1;
lookup gsub_contextual_1 {
    sub [one a]' lookup gsub_single_1 b' [two c]' lookup gsub_multiple_1;
} gsub_contextual_1;
lookup gsub_chain_contextual_1 {
    sub one two three a' lookup gsub_single_1 b' c' lookup gsub_multiple_1 x y z;
} gsub_chain_contextual_1;
lookup gsub_single_2 {
    sub [a b] by [b c];
} gsub_single_2;
lookup gsub_single_3 {
    sub [a b] by [d c];
} gsub_single_3;
lookup gsub_single_4 {
    sub a by c;
} gsub_single_4;
lookup gsub_chain_contextual_2 {
    sub one a' lookup gsub_single_4;
} gsub_chain_contextual_2;
lookup gsub_multiple_2 {
    sub e by NULL;
} gsub_multiple_2;
lookup gsub_chain_contextual_3 {
    ignore sub one a' b;
    sub a' lookup gsub_single_1;
} gsub_chain_contextual_3;
lookup gsub_contextual_2 {
    ignore sub a' b';
    sub a' lookup gsub_single_1 c';
} gsub_contextual_2;
lookup gsub_single_5 {
    lookupflag UseMarkFilteringSet [acute];
    sub b by c;
} gsub_single_5;
lookup gsub_reverse_1 {
    rsub one two [a b]' c by [d e];
} gsub_reverse_1;
lookup gsub_single_6 {
    sub one by two;
} gsub_single_6;
lookup gpos_mark_to_base_1 {
    pos base A
        <anchor 150 100> mark @bottomleft
        <anchor -200 -200> mark @mark_class_1_1;
} gpos_mark_to_base_1;
lookup gpos_single_1 {
    pos A 10;
} gpos_single_1;
lookup gpos_chain_contextual_1 {
    pos one A' lookup gpos_single_1;
} gpos_chain_contextual_1;
lookup gpos_single_2 {
    pos A 20;
} gpos_single_2;
lookup gpos_chain_contextual_2 {
    ignore pos one A';
    pos A' lookup gpos_single_2;
} gpos_chain_contextual_2;
lookup gpos_single_3 {
    pos [a b] -50;
} gpos_single_3;
lookup gpos_single_4 {
    pos a -50;
    pos b -60;
} gpos_single_4;
lookup gpos_single_5 {
    pos one 30;
} gpos_single_5;
feature ss01 {
lookup gsub_single_6;
    lookup gpos_single_5;
} ss01;
"
        );
    }

    #[test]
    fn test_uncompile_language_systems() {
        let data = std::fs::read("resources/languages.ttf").unwrap();
        let fontref = skrifa::FontRef::new(&data).unwrap();
        let ff = uncompile(&fontref, true).unwrap();
        assert_eq!(
            ff.as_fea(""),
            "\
languagesystem DFLT dflt;
languagesystem latn dflt;
languagesystem latn TRK;
lookup gsub_single_1 {
    sub b by c;
} gsub_single_1;
lookup gsub_single_2 {
    sub a by d;
} gsub_single_2;
lookup gsub_single_3 {
    sub a by e;
} gsub_single_3;
feature calt {
script latn;
    lookup gsub_single_2;
    language TRK;
    lookup gsub_single_3;
} calt;
feature locl {
script latn;
    lookup gsub_single_2;
    language TRK exclude_dflt;
    lookup gsub_single_3;
} locl;
feature ss01 {
lookup gsub_single_1;
} ss01;
"
        );
    }

    #[test]
    fn test_uncompile_unregistered_features() {
        let data = std::fs::read("resources/unregistered.ttf").unwrap();
        let fontref = skrifa::FontRef::new(&data).unwrap();
        let ff = uncompile(&fontref, true).unwrap();
        assert_eq!(
            ff.as_fea(""),
            "\
languagesystem DFLT dflt;
lookup gsub_single_1 {
    sub b by c;
} gsub_single_1;
lookup gsub_single_2 {
    sub b by d;
} gsub_single_2;
feature ss01 {
lookup gsub_single_1;
} ss01;
feature ss02 {
# lookup gsub_single_2;
} ss02;
feature ss03 {

} ss03;
"
        );
    }

    #[test]
    fn test_uncompile_required_features() {
        let data = std::fs::read("resources/required.ttf").unwrap();
        let fontref = skrifa::FontRef::new(&data).unwrap();
        let ff = uncompile(&fontref, true).unwrap();
        assert_eq!(
            ff.as_fea(""),
            "\
languagesystem DFLT dflt;
lookup gsub_single_1 {
    sub a by b;
} gsub_single_1;
feature rlig {
script DFLT;
    language dflt required;
    lookup gsub_single_1;
} rlig;
"
        );
    }

    #[test]
    fn test_uncompile_ligature_carets() {
        let data = std::fs::read("resources/carets.ttf").unwrap();
        let fontref = skrifa::FontRef::new(&data).unwrap();
        let ff = uncompile(&fontref, true).unwrap();
        assert_eq!(
            ff.as_fea(""),
            "\
table GDEF {
LigatureCaretByPos a 100 200;
LigatureCaretByIndex b 3;
} GDEF;

"
        );
    }

    #[test]
    fn test_uncompile_empty_mark_attachment_class() {
        let data = std::fs::read("resources/markattach.ttf").unwrap();
        let fontref = skrifa::FontRef::new(&data).unwrap();
        let ff = uncompile(&fontref, true).unwrap();
        assert_eq!(
            ff.as_fea(""),
            "\
languagesystem DFLT dflt;
lookup gsub_single_1 {
    lookupflag MarkAttachmentType [];
    sub a by b;
} gsub_single_1;
feature ss01 {
lookup gsub_single_1;
} ss01;
"
        );
    }

    #[test]
    fn test_uncompile_attachment_points() {
        let data = std::fs::read("resources/attach.ttf").unwrap();
        let fontref = skrifa::FontRef::new(&data).unwrap();
        let ff = uncompile(&fontref, true).unwrap();
        assert_eq!(
            ff.as_fea(""),
            "\
table GDEF {
Attach a 1 2;
Attach b 3;
} GDEF;

"
        );
    }
}
