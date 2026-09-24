use crate::Features;
use fea_rs_ast::{
    AsFea as _, Comment, FeatureFile, GlyphClass, GlyphContainer, GlyphName, LayoutVisitor,
    Statement, SubOrPos,
};
use smol_str::SmolStr;
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

use crate::filters::FontFilter;

/// A filter that subsets the layout features of a font to only include specified glyphs
pub struct SubsetLayout(Vec<SmolStr>);

impl SubsetLayout {
    /// Create a new SubsetLayout filter
    pub fn new<T: Into<SmolStr>>(glyphs: Vec<T>) -> Self {
        SubsetLayout(glyphs.into_iter().map(|g| g.into()).collect())
    }
}

impl FontFilter for SubsetLayout {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let old_glyphs = font
            .glyphs
            .iter()
            .map(|g| g.name.as_str())
            .collect::<Vec<_>>();
        let new_glyphs = self
            .0
            .iter()
            .filter(|glyph| old_glyphs.contains(&glyph.as_str()))
            .map(|s| s.as_str())
            .collect::<Vec<_>>();
        let features = font.features.to_fea();
        let mut feature_file: FeatureFile =
            FeatureFile::new_from_fea(&features, Some(&old_glyphs), font.source.clone())?;
        let glyph_set: HashSet<&str> = new_glyphs.iter().copied().collect();
        let mut visitor = SubsetVisitor::new(glyph_set);
        visitor.visit(&mut feature_file).map_err(|e| {
            crate::BabelfontError::FilterError(format!("Error during feature subsetting: {}", e))
        })?;
        font.features = Features::from_fea(&feature_file.as_fea(""));
        Ok(())
    }

    fn from_str(s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        let glyphs = s.split(',').map(|s| SmolStr::new(s.trim())).collect();
        Ok(SubsetLayout(glyphs))
    }
    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg {
        clap::Arg::new("subsetlayout")
            .long("subset-layout")
            .value_name("GLYPHS")
            .help("Subset layout features to only include specified glyphs (comma-separated list)")
    }
}

static DELETION_COMMENT: LazyLock<Statement> = std::sync::LazyLock::new(|| {
    Statement::Comment(Comment::new(
        "# Removed statement due to no glyphs remaining".to_string(),
    ))
});
/// A visitor which subsets a feature file down to a given glyphset.
///
/// The [`SubsetLayout`] filter drives this for you. Callers who need to preseed
/// it -- for example to drop lookups whose names collide when merging fonts --
/// can construct it directly and use [`SubsetVisitor::drop_lookups`].
pub struct SubsetVisitor<'a> {
    glyphs: HashSet<&'a str>,
    dropped_lookups: HashSet<SmolStr>,
    dropped_features: HashSet<String>,
    empty_classes: HashSet<String>,
    /// The most recent definition of a name wins for every later reference. This maps
    /// an original lookup name to the name it is actually emitted under (usually the
    /// same; see [`SubsetVisitor::reserve_lookup_names`]).
    lookup_bindings: HashMap<SmolStr, SmolStr>,
    /// Every lookup name already in use, so that a renamed lookup gets a suffix which
    /// doesn't clash with anything else in the file.
    used_lookup_names: HashSet<SmolStr>,
    /// Mark classes which lost all of their glyphs to subsetting. Positioning
    /// rules that reference them have nothing left to attach and must go too.
    dropped_mark_classes: HashSet<SmolStr>,
    original_class_definitions: HashMap<SmolStr, Vec<SmolStr>>,
    /// FEA glyph classes are unscoped globals: the most recent definition of a
    /// name wins for every later reference. This maps an original class name to
    /// the name it is actually emitted under (usually the same; see
    /// [`SubsetVisitor::reserve_class_names`]).
    class_bindings: HashMap<SmolStr, SmolStr>,
    /// Every class name already in use, so that a renamed definition gets a
    /// suffix which doesn't clash with anything else in the file.
    used_class_names: HashSet<SmolStr>,
}
impl<'a> SubsetVisitor<'a> {
    /// Create a new `SubsetVisitor` with the specified set of glyphs.
    pub fn new(glyphs: HashSet<&'a str>) -> Self {
        Self {
            glyphs,
            dropped_lookups: HashSet::new(),
            dropped_features: HashSet::new(),
            empty_classes: HashSet::new(),
            lookup_bindings: HashMap::new(),
            used_lookup_names: HashSet::new(),
            dropped_mark_classes: HashSet::new(),
            original_class_definitions: HashMap::new(),
            class_bindings: HashMap::new(),
            used_class_names: HashSet::new(),
        }
    }

    /// Drop the specified lookups from the subsetting process. Any lookups added
    /// to this set will be considered as removed and will not appear in the
    /// resulting subsetted feature file.
    pub fn drop_lookups(&mut self, lookups: HashSet<SmolStr>) {
        self.dropped_lookups.extend(lookups);
    }

    /// Reserve lookup names that are already in use elsewhere, for example by the
    /// host font when merging under the "both" duplicate policy. A definition in
    /// this file which uses a reserved name is renamed with a numeric suffix
    /// (`foo` becomes `foo_2`, and so on) and any references to it are updated.
    pub fn reserve_lookup_names(&mut self, names: impl IntoIterator<Item = SmolStr>) {
        for name in names {
            self.used_lookup_names.insert(name.clone());
            self.lookup_bindings.entry(name.clone()).or_insert(name);
        }
    }

    /// Allocate the name a lookup should be emitted under. The first definition of
    /// a name keeps it; a later definition (or one whose name was reserved via
    /// [`SubsetVisitor::reserve_lookup_names`]) is suffixed `_2`, `_3`, ... .
    fn bind_lookup_name(&mut self, original: &str) -> SmolStr {
        let original = SmolStr::new(original);
        if !self.used_lookup_names.contains(&original) {
            self.used_lookup_names.insert(original.clone());
            self.lookup_bindings
                .insert(original.clone(), original.clone());
            return original;
        }
        let mut index = 2usize;
        loop {
            let candidate = SmolStr::new(format!("{original}_{index}"));
            if !self.used_lookup_names.contains(&candidate) {
                self.used_lookup_names.insert(candidate.clone());
                self.lookup_bindings.insert(original, candidate.clone());
                return candidate;
            }
            index += 1;
        }
    }

    /// Reserve class names that are already in use elsewhere, for example by the
    /// host font when merging. A definition in this file which uses a reserved
    /// name is renamed with a numeric suffix (`@FOO` becomes `@FOO_2`, and so
    /// on) and any later references to it are updated to match.
    pub fn reserve_class_names(&mut self, names: impl IntoIterator<Item = SmolStr>) {
        for name in names {
            self.used_class_names.insert(name.clone());
            self.class_bindings.entry(name.clone()).or_insert(name);
        }
    }

    /// Allocate the name a class definition should be emitted under. The first
    /// definition of a name keeps it; any later definition (or one whose name
    /// was reserved via [`SubsetVisitor::reserve_class_names`]) is suffixed
    /// `_2`, `_3`, ... to keep it distinct.
    fn bind_class_name(&mut self, original: &str) -> SmolStr {
        let original = SmolStr::new(original);
        if !self.used_class_names.contains(&original) {
            self.used_class_names.insert(original.clone());
            self.class_bindings
                .insert(original.clone(), original.clone());
            return original;
        }
        let mut index = 2usize;
        loop {
            let candidate = SmolStr::new(format!("{original}_{index}"));
            if !self.used_class_names.contains(&candidate) {
                self.used_class_names.insert(candidate.clone());
                self.class_bindings.insert(original, candidate.clone());
                return candidate;
            }
            index += 1;
        }
    }

    fn expand_glyph_container(&self, gc: &GlyphContainer) -> Vec<SmolStr> {
        // Expand original glyphs recursively
        let mut todo = vec![gc.clone()];
        let mut original_glyphs = vec![];
        while let Some(container) = todo.pop() {
            match container {
                GlyphContainer::GlyphName(glyph_name) => {
                    original_glyphs.push(glyph_name.name.clone());
                }
                GlyphContainer::GlyphClassName(mut class_name) => {
                    if class_name.starts_with("@") {
                        class_name = class_name[1..].into();
                    }
                    // Resolve through any rename before looking the class up.
                    let effective = self
                        .class_bindings
                        .get(&class_name)
                        .cloned()
                        .unwrap_or(class_name);
                    if let Some(definition) = self.original_class_definitions.get(&effective) {
                        for glyph in definition.iter().rev() {
                            todo.push(GlyphContainer::GlyphName(GlyphName::new(glyph)));
                        }
                    } else {
                        log::warn!("Warning: no definition found for glyph class {}", effective);
                    }
                }
                GlyphContainer::GlyphClass(glyph_class) => {
                    for gc in glyph_class.glyphs.iter().rev() {
                        todo.push(gc.clone());
                    }
                }
                GlyphContainer::GlyphNameOrRange(name) => {
                    // I'm just going to treat it as a glyph name for now
                    original_glyphs.push(name.clone());
                }
                GlyphContainer::GlyphRange(range) => {
                    for glyph in range.glyphset() {
                        original_glyphs.push(glyph);
                    }
                }
            }
        }
        original_glyphs
    }

    fn subset_single_subst(
        &self,
        statement: &mut fea_rs_ast::SingleSubstStatement,
    ) -> Option<Statement> {
        for vec_container in [statement.prefix.iter_mut(), statement.suffix.iter_mut()] {
            for container in vec_container {
                if !self.filter_container(container) {
                    return Some(DELETION_COMMENT.clone());
                }
            }
        }
        // We have to go pairwise over glyph->replacement,
        // looking into class definitions as we do so.
        let mapping_from: Vec<SmolStr> = statement
            .glyphs
            .iter()
            .flat_map(|gc| self.expand_glyph_container(gc))
            .collect::<Vec<_>>();
        let mapping_to: Vec<SmolStr> = statement
            .replacement
            .iter()
            .flat_map(|gc| self.expand_glyph_container(gc))
            .collect::<Vec<_>>();
        let mapping = mapping_from.into_iter().zip(mapping_to);
        // Empty the existing mapping
        statement.glyphs.clear();
        statement.replacement.clear();
        let mut new_from = vec![];
        let mut new_to = vec![];

        for (glyph, replacement) in mapping {
            if self.glyphs.contains(glyph.as_str()) && self.glyphs.contains(replacement.as_str()) {
                new_from.push(GlyphContainer::GlyphName(GlyphName::new(glyph.as_str())));
                new_to.push(GlyphContainer::GlyphName(GlyphName::new(
                    replacement.as_str(),
                )));
            }
        }
        match new_from.len() {
            0 => Some(DELETION_COMMENT.clone()),
            1 => Some(Statement::SingleSubst(fea_rs_ast::SingleSubstStatement {
                prefix: statement.prefix.clone(),
                suffix: statement.suffix.clone(),
                glyphs: new_from,
                replacement: new_to,
                location: statement.location.clone(),
                force_chain: statement.force_chain,
            })),
            _ => {
                // Put them into classes
                Some(Statement::SingleSubst(fea_rs_ast::SingleSubstStatement {
                    prefix: statement.prefix.clone(),
                    suffix: statement.suffix.clone(),
                    glyphs: vec![GlyphContainer::GlyphClass(GlyphClass::new(
                        new_from,
                        statement.location.clone(),
                    ))],
                    replacement: vec![GlyphContainer::GlyphClass(GlyphClass::new(
                        new_to,
                        statement.location.clone(),
                    ))],
                    location: statement.location.clone(),
                    force_chain: statement.force_chain,
                }))
            }
        }
    }
    fn subset_multiple_subst(
        &self,
        statement: &mut fea_rs_ast::MultipleSubstStatement,
    ) -> Option<Statement> {
        if !self.filter_container(&mut statement.glyph) {
            return Some(DELETION_COMMENT.clone());
        }

        for vec_container in [
            statement.replacement.iter_mut(),
            statement.prefix.iter_mut(),
            statement.suffix.iter_mut(),
        ] {
            for container in vec_container {
                if !self.filter_container(container) {
                    return Some(DELETION_COMMENT.clone());
                }
            }
        }
        None
    }
    fn subset_alternate_subst(
        &self,
        statement: &mut fea_rs_ast::AlternateSubstStatement,
    ) -> Option<Statement> {
        if !self.filter_container(&mut statement.glyph) {
            return Some(DELETION_COMMENT.clone());
        }
        if !self.filter_container(&mut statement.replacement) {
            return Some(DELETION_COMMENT.clone());
        }

        for vec_container in [statement.prefix.iter_mut(), statement.suffix.iter_mut()] {
            for container in vec_container {
                if !self.filter_container(container) {
                    return Some(DELETION_COMMENT.clone());
                }
            }
        }
        None
    }
    fn subset_ligature_subst(
        &self,
        statement: &mut fea_rs_ast::LigatureSubstStatement,
    ) -> Option<Statement> {
        if !self.filter_container(&mut statement.replacement) {
            return Some(DELETION_COMMENT.clone());
        }
        for vec_container in [
            statement.glyphs.iter_mut(),
            statement.prefix.iter_mut(),
            statement.suffix.iter_mut(),
        ] {
            for container in vec_container {
                if !self.filter_container(container) {
                    return Some(DELETION_COMMENT.clone());
                }
            }
        }
        None
    }
    fn subset_reverse_chain_single_subst(
        &self,
        statement: &mut fea_rs_ast::ReverseChainSingleSubstStatement,
    ) -> Option<Statement> {
        for vec_container in [statement.prefix.iter_mut(), statement.suffix.iter_mut()] {
            for container in vec_container {
                if !self.filter_container(container) {
                    return Some(DELETION_COMMENT.clone());
                }
            }
        }
        // XXX Code copied from single subst above
        let mapping_from: Vec<SmolStr> = statement
            .glyphs
            .iter()
            .flat_map(|gc| self.expand_glyph_container(gc))
            .collect::<Vec<_>>();
        let mapping_to: Vec<SmolStr> = statement
            .replacements
            .iter()
            .flat_map(|gc| self.expand_glyph_container(gc))
            .collect::<Vec<_>>();
        let mapping = mapping_from.into_iter().zip(mapping_to);
        // Empty the existing mapping
        statement.glyphs.clear();
        statement.replacements.clear();
        let mut new_from = vec![];
        let mut new_to = vec![];

        for (glyph, replacement) in mapping {
            if self.glyphs.contains(glyph.as_str()) && self.glyphs.contains(replacement.as_str()) {
                new_from.push(GlyphContainer::GlyphName(GlyphName::new(glyph.as_str())));
                new_to.push(GlyphContainer::GlyphName(GlyphName::new(
                    replacement.as_str(),
                )));
            }
        }
        match new_from.len() {
            0 => Some(DELETION_COMMENT.clone()),
            1 => Some(Statement::ReverseChainSubst(
                fea_rs_ast::ReverseChainSingleSubstStatement {
                    prefix: statement.prefix.clone(),
                    suffix: statement.suffix.clone(),
                    glyphs: new_from,
                    replacements: new_to,
                    location: statement.location.clone(),
                },
            )),
            _ => {
                // Put them into classes
                Some(Statement::ReverseChainSubst(
                    fea_rs_ast::ReverseChainSingleSubstStatement {
                        prefix: statement.prefix.clone(),
                        suffix: statement.suffix.clone(),
                        glyphs: vec![GlyphContainer::GlyphClass(GlyphClass::new(
                            new_from,
                            statement.location.clone(),
                        ))],
                        replacements: vec![GlyphContainer::GlyphClass(GlyphClass::new(
                            new_to,
                            statement.location.clone(),
                        ))],
                        location: statement.location.clone(),
                    },
                ))
            }
        }
    }
    fn subset_single_pos(
        &self,
        statement: &mut fea_rs_ast::SinglePosStatement,
    ) -> Option<Statement> {
        for vec_container in [statement.prefix.iter_mut(), statement.suffix.iter_mut()] {
            for container in vec_container {
                if !self.filter_container(container) {
                    return Some(DELETION_COMMENT.clone());
                }
            }
        }
        for (container, _vr) in statement.pos.iter_mut() {
            if !self.filter_container(container) {
                return Some(DELETION_COMMENT.clone());
            }
        }
        None
    }
    fn subset_pair_pos(&self, statement: &mut fea_rs_ast::PairPosStatement) -> Option<Statement> {
        if !self.filter_container(&mut statement.glyphs_1) {
            return Some(DELETION_COMMENT.clone());
        }
        if !self.filter_container(&mut statement.glyphs_2) {
            return Some(DELETION_COMMENT.clone());
        }
        None
    }
    fn subset_cursive_pos(
        &self,
        statement: &mut fea_rs_ast::CursivePosStatement,
    ) -> Option<Statement> {
        if !self.filter_container(&mut statement.glyphclass) {
            return Some(DELETION_COMMENT.clone());
        }
        None
    }
    fn subset_mark_base_pos(
        &self,
        statement: &mut fea_rs_ast::MarkBasePosStatement,
    ) -> Option<Statement> {
        if !self.filter_container(&mut statement.base) {
            return Some(DELETION_COMMENT.clone());
        }
        // Drop any (anchor, mark class) pairs whose mark class has emptied;
        // a rule with no marks left has nothing to position.
        statement
            .marks
            .retain(|(_, mark_class)| !self.dropped_mark_classes.contains(&mark_class.name));
        if statement.marks.is_empty() {
            return Some(DELETION_COMMENT.clone());
        }
        None
    }
    fn subset_mark_lig_pos(
        &self,
        statement: &mut fea_rs_ast::MarkLigPosStatement,
    ) -> Option<Statement> {
        if !self.filter_container(&mut statement.ligatures) {
            return Some(DELETION_COMMENT.clone());
        }
        None
    }
    fn subset_mark_mark_pos(
        &self,
        statement: &mut fea_rs_ast::MarkMarkPosStatement,
    ) -> Option<Statement> {
        if !self.filter_container(&mut statement.base_marks) {
            return Some(DELETION_COMMENT.clone());
        }
        None
    }
    fn subset_chained_context<T: SubOrPos>(
        &self,
        statement: &mut fea_rs_ast::ChainedContextStatement<T>,
    ) -> Option<Statement> {
        for vec_container in [
            statement.prefix.iter_mut(),
            statement.suffix.iter_mut(),
            statement.glyphs.iter_mut(),
        ] {
            for container in vec_container {
                if !self.filter_container(container) {
                    return Some(DELETION_COMMENT.clone());
                }
            }
        }
        // Rename any lookups this statement refers to, then drop the statement if
        // any of them have gone (it can no longer fire).
        for lookupset in statement.lookups.iter_mut() {
            for lookup in lookupset.iter_mut() {
                if let Some(effective) = self.lookup_bindings.get(lookup.as_str()) {
                    if effective.as_str() != lookup.as_str() {
                        *lookup = effective.clone();
                    }
                }
                if self.dropped_lookups.contains(&*lookup) {
                    return Some(DELETION_COMMENT.clone());
                }
            }
        }
        None
    }
    fn subset_ignore<T: SubOrPos>(
        &self,
        statement: &mut fea_rs_ast::IgnoreStatement<T>,
    ) -> Option<Statement> {
        let mut new_context = vec![];
        for context in statement.chain_contexts.iter_mut() {
            let mut include = true;
            for vec_container in [
                context.0.iter_mut(),
                context.1.iter_mut(),
                context.2.iter_mut(),
            ] {
                for container in vec_container {
                    if !self.filter_container(container) {
                        include = false;
                    }
                }
            }
            if include {
                new_context.push(context.clone());
            }
        }
        if new_context.is_empty() {
            return Some(DELETION_COMMENT.clone());
        }
        statement.chain_contexts = new_context;
        None
    }

    fn subset_mark_class_definition(
        &mut self,
        statement: &mut fea_rs_ast::MarkClassDefinition,
    ) -> Option<Statement> {
        if !self.filter_container(&mut statement.glyphs) {
            self.dropped_mark_classes
                .insert(statement.mark_class.name.clone());
            return Some(Statement::Comment(Comment::new(format!(
                "# Removed mark class definition {} due to no glyphs remaining",
                statement.mark_class.name
            ))));
        }
        None
    }
    fn subset_glyph_class_definition(
        &mut self,
        statement: &mut fea_rs_ast::GlyphClassDefinition,
    ) -> Option<Statement> {
        // Work out what this class should be called. If its name is already taken
        // -- because it was defined earlier in this file, or reserved by the host
        // font -- it gets a numbered suffix, so that we never emit two classes
        // sharing a name.
        let effective_name = self.bind_class_name(&statement.name);
        statement.name = effective_name.to_string();

        // Store the original (pre-filtering) class definition under its emitted
        // name, so that references can be expanded back to glyphs later.
        let original_glyphs = statement
            .glyphs
            .glyphs
            .iter()
            .flat_map(|gc| self.expand_glyph_container(gc))
            .collect();
        self.original_class_definitions
            .insert(effective_name.clone(), original_glyphs);

        statement
            .glyphs
            .glyphs
            .retain_mut(|container| self.filter_container(container));

        if statement.glyphs.glyphs.is_empty() {
            self.empty_classes.insert(format!("@{effective_name}"));
            return Some(Statement::Comment(Comment::new(format!(
                "# Removed glyph class {} due to no glyphs remaining",
                effective_name
            ))));
        }
        None
    }
    fn subset_gdef_class_definition(
        &mut self,
        statement: &mut fea_rs_ast::GlyphClassDefStatement,
    ) -> Option<Statement> {
        if let Some(container) = &mut statement.base_glyphs {
            self.filter_container(container);
        }
        if let Some(container) = &mut statement.mark_glyphs {
            self.filter_container(container);
        }
        if let Some(container) = &mut statement.ligature_glyphs {
            self.filter_container(container);
        }
        if let Some(container) = &mut statement.component_glyphs {
            self.filter_container(container);
        }

        None
    }
    fn subset_gdef_attach(
        &mut self,
        statement: &mut fea_rs_ast::AttachStatement,
    ) -> Option<Statement> {
        if !self.filter_container(&mut statement.glyphs) {
            return Some(Statement::Comment(Comment::new(
                "# Removed GDEF attach statement due to no glyphs remaining".to_string(),
            )));
        }
        None
    }
    fn subset_gdef_ligature_caret_by_index(
        &mut self,
        statement: &mut fea_rs_ast::LigatureCaretByIndexStatement,
    ) -> Option<Statement> {
        if !self.filter_container(&mut statement.glyphs) {
            return Some(Statement::Comment(Comment::new(
                "# Removed GDEF ligature caret by index statement due to no glyphs remaining"
                    .to_string(),
            )));
        }
        None
    }
    fn subset_gdef_ligature_caret_by_pos(
        &mut self,
        statement: &mut fea_rs_ast::LigatureCaretByPosStatement,
    ) -> Option<Statement> {
        if !self.filter_container(&mut statement.glyphs) {
            return Some(Statement::Comment(Comment::new(
                "# Removed GDEF ligature caret by pos statement due to no glyphs remaining"
                    .to_string(),
            )));
        }
        None
    }
    fn subset_lookupflag(
        &mut self,
        lookupflag: &mut fea_rs_ast::LookupFlagStatement,
    ) -> Option<Statement> {
        let empty_class =
            GlyphContainer::GlyphClass(GlyphClass::new(vec![], lookupflag.location.clone()));
        for container in [
            &mut lookupflag.mark_attachment,
            &mut lookupflag.mark_filtering_set,
        ]
        .into_iter()
        .flatten()
        {
            match container {
                GlyphContainer::GlyphClass(_) => {
                    self.filter_container(container);
                }
                GlyphContainer::GlyphClassName(name)
                    if self.empty_classes.contains(name.as_str()) =>
                {
                    *container = empty_class.clone();
                }
                _ => {}
            }
        }
        None
    }
    fn subset_feature_block(
        &mut self,
        feature_block: &mut fea_rs_ast::FeatureBlock,
    ) -> Option<Statement> {
        feature_block
            .statements
            .retain(|statement| statement != &*DELETION_COMMENT);
        if feature_block.statements.iter().any(non_trivial_statement) {
            return None;
        }
        self.dropped_features.insert(feature_block.name.to_string());
        Some(Statement::Comment(Comment::new(format!(
            "# Removed feature {} due to no statements remaining",
            feature_block.name
        ))))
    }
    fn subset_lookup_block(
        &mut self,
        lookup_block: &mut fea_rs_ast::LookupBlock,
    ) -> Option<Statement> {
        // A lookup whose name is already in `dropped_lookups` has been superseded by
        // a lookup with the same name (typically one that came from the host font),
        // so it goes wholesale, however many statements it still contains. References
        // to it are handled by `subset_lookup_reference`.
        if self.dropped_lookups.contains(&lookup_block.name) {
            return Some(Statement::Comment(Comment::new(format!(
                "# Removed lookup {} because a lookup with that name already exists",
                lookup_block.name
            ))));
        }
        // Otherwise give it its own name: one reserved by the caller (e.g. one the
        // host font already defines, under the "both" duplicate policy) is suffixed
        // so that the two can coexist, and references are updated to match.
        let effective_name = self.bind_lookup_name(&lookup_block.name);
        lookup_block.name = effective_name.clone();

        lookup_block
            .statements
            .retain(|statement| statement != &*DELETION_COMMENT);
        if lookup_block.statements.iter().any(non_trivial_statement) {
            return None;
        }
        self.dropped_lookups.insert(effective_name.clone());
        Some(Statement::Comment(Comment::new(format!(
            "# Removed lookup {} due to no statements remaining",
            effective_name
        ))))
    }
    fn subset_feature_reference(
        &mut self,
        feature_reference: &mut fea_rs_ast::FeatureReferenceStatement,
    ) -> Option<Statement> {
        if self
            .dropped_features
            .contains(&feature_reference.feature_name)
        {
            return Some(Statement::Comment(Comment::new(format!(
                "# Removed feature reference to {} due to feature being dropped",
                feature_reference.feature_name
            ))));
        }
        None
    }
    fn subset_lookup_reference(
        &mut self,
        lookup_reference: &mut fea_rs_ast::LookupReferenceStatement,
    ) -> Option<Statement> {
        // Update the reference if the lookup it points at has been renamed.
        if let Some(effective) = self
            .lookup_bindings
            .get(lookup_reference.lookup_name.as_str())
        {
            if effective.as_str() != lookup_reference.lookup_name {
                lookup_reference.lookup_name = effective.to_string();
            }
        }
        if self
            .dropped_lookups
            .contains(&SmolStr::from(lookup_reference.lookup_name.clone()))
        {
            return Some(Statement::Comment(Comment::new(format!(
                "# Removed lookup reference to {} due to lookup being dropped",
                lookup_reference.lookup_name
            ))));
        }
        None
    }
    fn subset_nested_block(
        &mut self,
        nested_block: &mut fea_rs_ast::NestedBlock,
    ) -> Option<Statement> {
        nested_block
            .statements
            .retain(|statement| statement != &*DELETION_COMMENT);
        // A featureNames (or sizemenuname) block is nothing BUT name records;
        // they are its payload, not scaffolding, and subsetting glyphs is no
        // reason to lose the font's UI strings.
        if nested_block
            .statements
            .iter()
            .any(|st| non_trivial_statement(st) || matches!(st, Statement::FeatureNameStatement(_)))
        {
            return None;
        }
        Some(Statement::Comment(Comment::new(
            "# Removed nested block due to no statements remaining".to_string(),
        )))
    }
    fn filter_container(&self, container: &mut fea_rs_ast::GlyphContainer) -> bool {
        match container {
            fea_rs_ast::GlyphContainer::GlyphName(glyph_name) => {
                self.glyphs.contains(glyph_name.name.as_str())
            }
            fea_rs_ast::GlyphContainer::GlyphClass(glyph_class) => {
                glyph_class
                    .glyphs
                    .retain_mut(|gc| self.filter_container(gc));
                !glyph_class.glyphs.is_empty()
            }
            fea_rs_ast::GlyphContainer::GlyphClassName(smol_str) => {
                // Point the reference at whatever name the class is emitted under.
                // Bindings are keyed without the leading `@`, references carry it.
                if let Some(effective) = self
                    .class_bindings
                    .get(smol_str.strip_prefix('@').unwrap_or(smol_str.as_str()))
                {
                    let renamed = SmolStr::new(format!("@{effective}"));
                    if *smol_str != renamed {
                        *smol_str = renamed;
                    }
                }
                !self.empty_classes.contains(smol_str.as_str())
            }
            fea_rs_ast::GlyphContainer::GlyphRange(range) => {
                *container = fea_rs_ast::GlyphContainer::GlyphClass(fea_rs_ast::GlyphClass::new(
                    range
                        .glyphset()
                        .map(|x| fea_rs_ast::GlyphContainer::GlyphName(GlyphName::new(&x)))
                        .collect(),
                    0..0, // Oops, we don't know
                ));
                self.filter_container(container)
            }
            fea_rs_ast::GlyphContainer::GlyphNameOrRange(smol_str) => {
                if self.glyphs.contains(smol_str.as_str()) {
                    return true;
                }
                // try interpreting as range
                todo!();
            }
        }
    }
}

fn non_trivial_statement(statement: &Statement) -> bool {
    // Does this statement give its enclosing block a reason to exist, or is it
    // just scaffolding (comments, script/language switches, lookupflags,
    // subtable markers, ...) that can be left behind when the rules go?
    match statement {
        // A reference is a block's payload, not scaffolding: a feature block
        // whose rules all live in referenced lookups (or whose variants are
        // assembled by `aalt`-style feature references) does something.
        // References to dropped lookups/features have already been turned into
        // comments by `subset_lookup_reference`/`subset_feature_reference`, so
        // any reference still here points at something live and must keep its
        // block alive.
        Statement::LookupReference(_) | Statement::FeatureReference(_) => true,
        // A featureNames (or sizemenuname) block is nothing but name records;
        // on its own it is no reason to keep the enclosing feature, whose rules
        // may have just been subsetted away.
        Statement::NestedBlock(_) => false,
        // Comments are normally scaffolding too, but the ufo2ft "Automatic
        // Code" marker tells a consumer where to inject generated rules, so it
        // must survive -- and, like a rule, it keeps its feature alive.
        Statement::Comment(comment) => comment.text.trim_start().starts_with("# Automatic Code"),
        Statement::FeatureNameStatement(_)
        | Statement::FontRevision(_)
        | Statement::Language(_)
        | Statement::LanguageSystem(_)
        | Statement::LookupFlag(_)
        | Statement::SizeParameters(_)
        | Statement::SizeMenuName(_)
        | Statement::Subtable(_)
        | Statement::Script(_)
        | Statement::Head(_) => false,
        _ => true,
    }
}
impl LayoutVisitor for SubsetVisitor<'_> {
    fn depth_first(&self) -> bool {
        true
    }
    fn visit_statement(&mut self, statement: &mut Statement) -> bool {
        if let Some(rewritten) = match statement {
            Statement::SingleSubst(single_subst_statement) => {
                self.subset_single_subst(single_subst_statement)
            }
            Statement::MultipleSubst(multiple_subst_statement) => {
                self.subset_multiple_subst(multiple_subst_statement)
            }
            Statement::AlternateSubst(alternate_subst_statement) => {
                self.subset_alternate_subst(alternate_subst_statement)
            }
            Statement::LigatureSubst(ligature_subst_statement) => {
                self.subset_ligature_subst(ligature_subst_statement)
            }
            Statement::ReverseChainSubst(reverse_chain_single_subst_statement) => {
                self.subset_reverse_chain_single_subst(reverse_chain_single_subst_statement)
            }
            Statement::ChainedContextSubst(chained_context_statement) => {
                self.subset_chained_context(chained_context_statement)
            }
            Statement::IgnoreSubst(ignore_statement) => self.subset_ignore(ignore_statement),
            Statement::SinglePos(single_pos_statement) => {
                self.subset_single_pos(single_pos_statement)
            }
            Statement::PairPos(pair_pos_statement) => self.subset_pair_pos(pair_pos_statement),
            Statement::CursivePos(cursive_pos_statement) => {
                self.subset_cursive_pos(cursive_pos_statement)
            }
            Statement::MarkBasePos(mark_base_pos_statement) => {
                self.subset_mark_base_pos(mark_base_pos_statement)
            }
            Statement::MarkLigPos(mark_lig_pos_statement) => {
                self.subset_mark_lig_pos(mark_lig_pos_statement)
            }
            Statement::MarkMarkPos(mark_mark_pos_statement) => {
                self.subset_mark_mark_pos(mark_mark_pos_statement)
            }
            Statement::ChainedContextPos(chained_context_statement) => {
                self.subset_chained_context(chained_context_statement)
            }
            Statement::IgnorePos(ignore_statement) => self.subset_ignore(ignore_statement),
            Statement::AnchorDefinition(_) => None,
            Statement::GdefAttach(attach_statement) => self.subset_gdef_attach(attach_statement),
            Statement::GdefClassDef(glyph_class_def_statement) => {
                self.subset_gdef_class_definition(glyph_class_def_statement)
            }
            Statement::GdefLigatureCaretByIndex(ligature_caret_by_index_statement) => {
                self.subset_gdef_ligature_caret_by_index(ligature_caret_by_index_statement)
            }
            Statement::GdefLigatureCaretByPos(ligature_caret_by_pos_statement) => {
                self.subset_gdef_ligature_caret_by_pos(ligature_caret_by_pos_statement)
            }
            Statement::MarkClassDefinition(mark_class_definition) => {
                self.subset_mark_class_definition(mark_class_definition)
            }
            Statement::Comment(_)
            | Statement::FeatureNameStatement(_)
            | Statement::FontRevision(_) => None,
            Statement::FeatureReference(feature_reference) => {
                self.subset_feature_reference(feature_reference)
            }
            Statement::GlyphClassDefinition(glyph_class_definition) => {
                self.subset_glyph_class_definition(glyph_class_definition)
            }
            Statement::Language(_) | Statement::LanguageSystem(_) => None,
            Statement::LookupFlag(lookupflag) => self.subset_lookupflag(lookupflag),
            Statement::LookupReference(lookup_reference) => {
                self.subset_lookup_reference(lookup_reference)
            }
            Statement::SizeParameters(_)
            | Statement::SizeMenuName(_)
            | Statement::Subtable(_)
            | Statement::Script(_) => None,
            Statement::Gdef(_) => {
                // Visitor will recurse
                None
            }
            Statement::Head(_)
            | Statement::Hhea(_)
            | Statement::Name(_)
            | Statement::Stat(_)
            | Statement::Vhea(_)
            | Statement::Os2(_)
            | Statement::Base(_) => None,
            Statement::FeatureBlock(feature_block) => self.subset_feature_block(feature_block),
            Statement::LookupBlock(lookup_block) => self.subset_lookup_block(lookup_block),
            Statement::NestedBlock(nested_block) => self.subset_nested_block(nested_block),
            Statement::ValueRecordDefinition(_) => None,
            Statement::ConditionSet(_) => None,
            Statement::VariationBlock(_) => None,
        } {
            *statement = rewritten;
            return true;
        }
        true
    }
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Font, Glyph};
    use pretty_assertions::assert_eq;

    fn dummy_font_with_glyphs(glyph_names: Vec<&str>) -> Font {
        let mut font = Font::new();
        for name in glyph_names {
            font.glyphs.push(Glyph::new(name));
        }
        font
    }

    #[test]
    fn test_subset_single_subst() {
        let mut font = dummy_font_with_glyphs(vec!["a", "b", "c"]);
        font.features = Features::from_fea(
            "feature foo { sub a by c; sub b by c; } foo;\nfeature bar { sub b by a; } bar;\n",
        );
        // Now subset to a and c only
        SubsetLayout::new(vec!["a", "c"])
            .apply(&mut font)
            .expect("Feature subsetting failed");
        let fea = font.features.to_fea();
        assert_eq!(fea, "feature foo {\nsub a by c;\n} foo;\n# Removed feature bar due to no statements remaining\n\n");
    }

    #[test]
    fn test_feature_of_lookup_references_survives() {
        // The SFD convertor emits features as script/language/lookup-reference
        // triples with the rules living in lookup prefixes. None of that may be
        // dropped as trivial while the referenced lookup is alive -- that empties
        // the font's FeatureList. A reference to a dropped lookup is still cleaned
        // away, and a feature left with nothing else still goes.
        let mut font = dummy_font_with_glyphs(vec!["a", "b", "c"]);
        font.features = Features::from_fea(
            "lookup one { sub a by c; } one;\nlookup two { sub b by c; } two;\n\
             feature foo { script DFLT; language dflt; lookup one; } foo;\n\
             feature bar { script DFLT; language dflt; lookup two; } bar;\n",
        );
        // Subset away b: lookup two empties, so bar loses its only payload.
        SubsetLayout::new(vec!["a", "c"])
            .apply(&mut font)
            .expect("Feature subsetting failed");
        let fea = font.features.to_fea();
        assert!(
            fea.contains("lookup one;"),
            "foo must survive on the strength of its lookup reference:\n{fea}"
        );
        assert!(
            fea.contains("# Removed feature bar"),
            "bar's lookup was dropped, so bar must still go:\n{fea}"
        );
    }

    #[test]
    fn test_featurenames_block_survives_subsetting() {
        // featureNames blocks hold nothing but name records, which the triviality
        // list treats as scaffolding -- but they are the block's payload, and
        // subsetting glyphs is no reason to drop the font's UI strings.
        let mut font = dummy_font_with_glyphs(vec!["a", "b", "c"]);
        font.features = Features::from_fea(
            "feature ss01 { featureNames { name 3 1 1033 \"Fancy\"; }; sub a by c; } ss01;\n",
        );
        SubsetLayout::new(vec!["a", "c"])
            .apply(&mut font)
            .expect("Feature subsetting failed");
        let fea = font.features.to_fea();
        assert!(
            fea.contains("featureNames") && fea.contains("name \"Fancy\";"),
            "the name record must survive (3/1/1033 renders as the elided default):\n{fea}"
        );
    }

    #[test]
    fn test_filter_range() {
        let visitor = SubsetVisitor::new(vec!["a", "b", "g"].into_iter().collect());
        let mut container = fea_rs_ast::GlyphContainer::GlyphRange(fea_rs_ast::GlyphRange::new(
            "a".into(),
            "f".into(),
        ));

        let retained = visitor.filter_container(&mut container);
        assert!(retained);
        assert_eq!(container.as_fea(""), "[a b]");
    }

    #[test]
    fn test_multiple_subst_with_classes() {
        let mut font = dummy_font_with_glyphs(vec!["a", "b", "c", "d"]);
        font.features = Features::from_fea(
            "@before = [a b]; @after = [c d]; feature foo { sub @before by @after; } foo;\n",
        );
        // Now subset to a and c only
        SubsetLayout::new(vec!["a", "c"])
            .apply(&mut font)
            .expect("Feature subsetting failed");
        let fea = font.features.to_fea();
        assert_eq!(
            fea,
            "@before = [a];\n@after = [c];\nfeature foo {\nsub a by c;\n} foo;\n\n"
        );
    }

    #[test]
    fn test_multiple_subset_retains_classes() {
        let all_glyphs = vec![
            "heh-ar.isol",
            "heh-ar.fina",
            "hamzaabove-ar",
            "heh-ar.isol.1",
            "heh-ar.fina.1",
        ];
        let mut font = dummy_font_with_glyphs(all_glyphs.clone());
        let feature_code = "feature foo {
sub [heh-ar.isol heh-ar.fina]' hamzaabove-ar by [heh-ar.isol.1 heh-ar.fina.1];
} foo;\n";
        font.features = Features::from_fea(feature_code);
        // keep them all, just rewrite
        SubsetLayout::new(all_glyphs.clone())
            .apply(&mut font)
            .expect("Feature subsetting failed");
        // Should be same
        let fea = font.features.to_fea();
        assert_eq!(fea.trim_end(), feature_code.trim_end());
    }
    use crate::{
        close_layout,
        convertors::fontir::{BabelfontIrSource, CompilationOptions},
    };

    #[test]
    fn test_fustat_subset() {
        let mut font = crate::load("resources/Fustat.glyphs").unwrap();
        let subset = [
            "fathatan-ar",
            "alef-ar.short.fina",
            "dotbelow-ar",
            "behDotless-ar.medi",
            "fatha-ar",
            "hah-ar.init",
            "reh-ar.fina",
            "meem-ar.init",
        ];
        // Perform layout closure
        let new_glyphset =
            close_layout(&font, subset.iter().map(|s| (*s).into()).collect()).unwrap();
        // Now subset to that glyphset
        SubsetLayout::new(new_glyphset.into_iter().collect())
            .apply(&mut font)
            .expect("Feature subsetting failed");
        // Just check that the resulting fea compiles
        BabelfontIrSource::compile(font, CompilationOptions::default()).unwrap();
    }

    #[test]
    fn test_cull_unwanted_named_features() {
        // A feature whose only surviving statement is a featureNames block is
        // still empty -- the name records are not rules. It must be culled, and
        // its UI strings with it.
        let mut font = dummy_font_with_glyphs(vec!["a", "a.alt", "b"]);
        font.features = Features::from_fea(
            "feature ss01 {\n\
             featureNames {\n\
             name \"Single story a\";\n\
             };\n\
             sub a by a.alt;\n\
             } ss01;\n",
        );
        SubsetLayout::new(vec!["b"])
            .apply(&mut font)
            .expect("Feature subsetting failed");
        let fea = font.features.to_fea();
        assert!(
            !fea.contains("feature ss01 {"),
            "ss01 has no rules left and must be dropped:\n{fea}"
        );
        assert!(
            !fea.contains("Single story a"),
            "the dropped feature's name record must go with it:\n{fea}"
        );
    }

    #[test]
    fn test_cull_unwanted_aalt() {
        // `aalt` assembles the wanted variants by reference. When one referenced
        // feature drops out, only that reference goes with it; a reference to a
        // live feature is payload and keeps `aalt` itself alive.
        let mut font = dummy_font_with_glyphs(vec!["a", "a.alt", "b", "b.alt"]);
        font.features = Features::from_fea(
            "feature ss01 {\n\
             featureNames {\n\
             name \"Single story a\";\n\
             };\n\
             sub a by a.alt;\n\
             } ss01;\n\
             feature ss02 {\n\
             sub b by b.alt;\n\
             } ss02;\n\
             feature aalt {\n\
             feature ss01;\n\
             feature ss02;\n\
             } aalt;\n",
        );
        SubsetLayout::new(vec!["b", "b.alt"])
            .apply(&mut font)
            .expect("Feature subsetting failed");
        let fea = font.features.to_fea();
        assert!(
            fea.contains("feature aalt {"),
            "aalt still references live ss02 and must survive:\n{fea}"
        );
        assert!(
            !fea.contains("feature ss01 {"),
            "ss01 has no glyphs left and must be dropped:\n{fea}"
        );
        assert!(
            fea.contains("feature ss02 {"),
            "ss02's glyphs were kept and it must survive:\n{fea}"
        );
    }

    #[test]
    fn test_both_ss_names() {
        // Two named stylistic sets: subsetting to a glyphset that keeps both must
        // preserve both blocks and both featureNames records.
        let mut font = dummy_font_with_glyphs(vec!["a", "a.alt", "g", "g.alt"]);
        font.features = Features::from_fea(
            "feature ss01 {\n\
             featureNames {\n\
             name \"Single story a\";\n\
             };\n\
             sub a by a.alt;\n\
             } ss01;\n\
             feature ss02 {\n\
             featureNames {\n\
             name \"Single story g\";\n\
             };\n\
             sub g by g.alt;\n\
             } ss02;\n",
        );
        SubsetLayout::new(vec!["a", "a.alt", "g", "g.alt"])
            .apply(&mut font)
            .expect("Feature subsetting failed");
        let fea = font.features.to_fea();
        assert!(fea.contains("feature ss01 {"), "{fea}");
        assert!(fea.contains("feature ss02 {"), "{fea}");
        assert!(fea.contains("Single story a"), "{fea}");
        assert!(fea.contains("Single story g"), "{fea}");
    }

    #[test]
    fn test_keep_automatic_code_comment() {
        // The ufo2ft "Automatic Code" marker must survive the fea-rs round trip,
        // and -- even with no rules left beside it -- must keep its feature alive.
        let mut font = dummy_font_with_glyphs(vec!["a", "b"]);
        font.features = Features::from_fea("feature mark {\n# Automatic Code\n} mark;\n");
        SubsetLayout::new(vec!["a"])
            .apply(&mut font)
            .expect("Feature subsetting failed");
        let fea = font.features.to_fea();
        assert!(
            fea.contains("# Automatic Code"),
            "the Automatic Code marker must be preserved:\n{fea}"
        );
    }

    #[test]
    fn test_drop_mark_class() {
        // When a mark class loses all of its glyphs, rules that attach marks via
        // it have nothing left to position and must be dropped too.
        let mut font = dummy_font_with_glyphs(vec!["a", "c", "A"]);
        font.features = Features::from_fea(
            "@something = [ a c ];\n\
             markClass @something <anchor 100 200> @MC_above;\n\
             feature mark {\n\
             lookup MARK_BASE_above {\n\
             @bGC_A_above = [A];\n\
             pos base @bGC_A_above <anchor 150 200> mark @MC_above;\n\
             } MARK_BASE_above;\n\
             } mark;\n",
        );
        SubsetLayout::new(vec!["A"])
            .apply(&mut font)
            .expect("Feature subsetting failed");
        let fea = font.features.to_fea();
        assert!(
            !fea.contains("pos base"),
            "the rule's mark class emptied, so the rule must go:\n{fea}"
        );
        assert!(
            !fea.contains("markClass"),
            "the emptied mark class definition must go:\n{fea}"
        );
    }

    #[test]
    fn test_drop_preseeded_lookup_definitions() {
        // A lookup whose name is already in `dropped_lookups` when it is visited is
        // dropped outright, however many statements it still contains, and features
        // that only referenced it are culled. This is how fontmerge implements its
        // "first" duplicate-lookup policy: preseed the names the host font already
        // defines, then subset the donor.
        let glyphs = vec!["a", "b"];
        let mut feature_file = FeatureFile::new_from_fea(
            "lookup one { sub a by b; } one;\nfeature foo { lookup one; } foo;\n",
            Some(&glyphs),
            None::<std::path::PathBuf>,
        )
        .expect("Failed to parse features");
        let mut visitor = SubsetVisitor::new(glyphs.iter().copied().collect());
        visitor.drop_lookups(HashSet::from([SmolStr::new("one")]));
        visitor
            .visit(&mut feature_file)
            .expect("Feature subsetting failed");
        let fea = feature_file.as_fea("");
        assert!(
            !fea.contains("lookup one {"),
            "the preselected lookup must be dropped:\n{fea}"
        );
        assert!(
            !fea.contains("feature foo {"),
            "the feature that only referenced it must be dropped:\n{fea}"
        );
    }

    #[test]
    fn test_redefined_class_is_renamed() {
        // FEA class definitions are unscoped globals, so a second definition of
        // the same name clobbers the first. We must not emit two classes sharing
        // a name: the second gets a numbered suffix, and references after it are
        // updated to match.
        let mut font = dummy_font_with_glyphs(vec!["a", "b", "c", "d"]);
        font.features = Features::from_fea(
            "@FOO = [a b];\n\
             feature one { pos @FOO b -10; } one;\n\
             @FOO = [c d];\n\
             feature two { pos @FOO d -20; } two;\n",
        );
        SubsetLayout::new(vec!["a", "b", "c", "d"])
            .apply(&mut font)
            .expect("Feature subsetting failed");
        let fea = font.features.to_fea();
        assert!(
            fea.contains("@FOO = [a b];"),
            "the first definition keeps its name:\n{fea}"
        );
        assert!(
            fea.contains("@FOO_2 = [c d];"),
            "the redefinition is renamed:\n{fea}"
        );
        assert!(
            fea.contains("pos @FOO b -10;"),
            "a reference before the redefinition is unchanged:\n{fea}"
        );
        assert!(
            fea.contains("pos @FOO_2 d -20;"),
            "a reference after the redefinition is updated:\n{fea}"
        );
    }

    #[test]
    fn test_reserved_class_name_is_renamed() {
        // A class name reserved by the caller (because, say, the host font
        // already defines it) is suffixed on definition, and references updated.
        let glyphs = vec!["a", "b"];
        let mut feature_file = FeatureFile::new_from_fea(
            "@FOO = [a b];\nfeature one { pos @FOO b -10; } one;\n",
            Some(&glyphs),
            None::<std::path::PathBuf>,
        )
        .expect("Failed to parse features");
        let mut visitor = SubsetVisitor::new(glyphs.iter().copied().collect());
        visitor.reserve_class_names([SmolStr::new("FOO")]);
        visitor
            .visit(&mut feature_file)
            .expect("Feature subsetting failed");
        let fea = feature_file.as_fea("");
        assert!(
            fea.contains("@FOO_2 = [a b];"),
            "a reserved class name is suffixed:\n{fea}"
        );
        assert!(
            fea.contains("pos @FOO_2 b -10;"),
            "references are updated to the renamed class:\n{fea}"
        );
    }

    #[test]
    fn test_reserved_lookup_name_is_renamed() {
        // Under the "both" duplicate policy a reserved lookup name (one the host
        // font already defines) is suffixed rather than dropped, and references
        // are updated to match.
        let glyphs = vec!["a", "b"];
        let mut feature_file = FeatureFile::new_from_fea(
            "lookup one { sub a by b; } one;\nfeature foo { lookup one; } foo;\n",
            Some(&glyphs),
            None::<std::path::PathBuf>,
        )
        .expect("Failed to parse features");
        let mut visitor = SubsetVisitor::new(glyphs.iter().copied().collect());
        visitor.reserve_lookup_names([SmolStr::new("one")]);
        visitor
            .visit(&mut feature_file)
            .expect("Feature subsetting failed");
        let fea = feature_file.as_fea("");
        assert!(
            fea.contains("lookup one_2 {"),
            "a reserved lookup name is suffixed:\n{fea}"
        );
        assert!(
            !fea.contains("lookup one {"),
            "the original name is not reused:\n{fea}"
        );
        assert!(
            fea.contains("lookup one_2;"),
            "references are updated to the renamed lookup:\n{fea}"
        );
    }

    #[test]
    fn test_subset_use_mark_filtering_set() {
        let glyphs = vec!["a", "b"];
        let mut feature_file = FeatureFile::new_from_fea(
            "lookup one { lookupflag UseMarkFilteringSet [nuktaknda]; sub a by b; } one;",
            Some(&glyphs),
            None::<std::path::PathBuf>,
        )
        .expect("Failed to parse features");
        let mut visitor = SubsetVisitor::new(glyphs.iter().copied().collect());
        visitor
            .visit(&mut feature_file)
            .expect("Feature subsetting failed");
        let fea = feature_file
            .as_fea("")
            .replace("\n", " ")
            .replace("    ", "");
        assert_eq!(
            fea, "lookup one { lookupflag UseMarkFilteringSet []; sub a by b; } one; ",
            "glyphs not in the subset are removed from UseMarkFilteringSet:\n{fea}"
        );
    }
}
