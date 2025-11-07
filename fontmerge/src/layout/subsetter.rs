use std::{ops::Range, path::PathBuf};

use fea_rs::{
    Kind, NodeOrToken,
    typed::{AstNode as _, Feature, GlyphOrClass, LookupBlock},
};
use indexmap::IndexSet;

use crate::layout::{
    find_first_glyph_or_class, get_parse_tree, glyph_names, visitor::LayoutVisitor,
};

pub(crate) struct LayoutSubsetter<'a> {
    features: &'a babelfont::Features,
    glyphset_for_parse: &'a [&'a str],

    final_glyphset: &'a [&'a str],
    hidden_classes: &'a IndexSet<String>,
    dropped_lookups: IndexSet<String>,
    language_systems: IndexSet<(String, String)>,
    project_root: PathBuf,
}

impl LayoutSubsetter<'_> {
    pub fn new<'a>(
        features: &'a babelfont::Features,
        glyphset_for_parse: &'a [&'a str],
        final_glyphset: &'a [&'a str],
        hidden_classes: &'a IndexSet<String>,
        dropped_lookups: &'a IndexSet<String>,
        project_root: impl Into<PathBuf>,
    ) -> LayoutSubsetter<'a> {
        LayoutSubsetter {
            features,
            glyphset_for_parse,
            final_glyphset,
            hidden_classes,
            dropped_lookups: dropped_lookups.clone(),
            project_root: project_root.into(),
            language_systems: IndexSet::new(),
        }
    }

    pub fn subset(&mut self) -> Result<babelfont::Features, crate::error::FontmergeError> {
        // First gather all language systems from the features
        for code in self.features.prefixes.values() {
            let parsed =
                get_parse_tree(code, self.glyphset_for_parse, &self.project_root).expect("foo");

            // These things are all at top level, we don't need a visitor
            for node in parsed
                .root()
                .iter_children()
                .filter(|n| n.kind() == fea_rs::Kind::LanguageSystemNode)
                .flat_map(|n| n.as_node())
            {
                let nodes = node
                    .iter_children()
                    .filter(|x| x.kind() == fea_rs::Kind::Tag)
                    .flat_map(|n| n.token_text())
                    .collect::<Vec<_>>();
                // We are assured there are two of them
                #[allow(clippy::indexing_slicing)]
                let (script, language) = (nodes[0].to_string(), nodes[1].to_string());
                log::debug!("Found language system: {} {}", script, language);
                self.language_systems.insert((script, language));
            }
        }
        // Run the subset visitor. Ideally we'd do it per prefix/feature/etc. but it's
        // complicated as we have to store all references first then rewrite later.
        let features = self.features.to_fea();
        let mut visitor = LayoutSubsetVisitor::new(
            self.final_glyphset,
            self.hidden_classes,
            &self.dropped_lookups,
            get_parse_tree(&features, self.glyphset_for_parse, &self.project_root)?,
        );

        visitor.visit();
        let mut features = babelfont::Features::default();
        features
            .prefixes
            .insert("anonymous".into(), visitor.do_subset());
        Ok(features)
    }
}

struct LayoutSubsetVisitor<'a> {
    final_glyphset: &'a [&'a str],
    class_name_references: std::collections::HashMap<String, Vec<fea_rs::Node>>,
    dropped_lookups: IndexSet<String>,
    dropped_features: IndexSet<String>,
    referenced_mark_classes: IndexSet<String>,
    hidden_classes: IndexSet<String>,
    pub parse_tree: fea_rs::ParseTree,
    replacement_list: Vec<(Range<usize>, fea_rs::Node)>,
}

impl LayoutSubsetVisitor<'_> {
    pub fn new<'a>(
        final_glyphset: &'a [&'a str],
        hidden_classes: &'a IndexSet<String>,
        dropped_lookups: &'a IndexSet<String>,
        parse_tree: fea_rs::ParseTree,
    ) -> LayoutSubsetVisitor<'a> {
        LayoutSubsetVisitor {
            final_glyphset,
            class_name_references: std::collections::HashMap::new(),
            dropped_lookups: dropped_lookups.clone(),
            dropped_features: IndexSet::new(),
            referenced_mark_classes: IndexSet::new(),
            hidden_classes: hidden_classes.clone(),
            parse_tree,
            replacement_list: Vec::new(),
        }
    }
    fn nothing() -> fea_rs::Node {
        let (node, _) = fea_rs::parse::parse_string("");
        node.root().clone()
    }

    pub fn do_subset(&mut self) -> String {
        // Apply deletions to the parse tree and reconstruct features
        let edits = std::mem::take(&mut self.replacement_list);
        let mut new_tree = self.parse_tree.root().edit(edits, false);
        // let _: Vec<_> = new_tree.iter_tokens().collect();

        // Now the parse tree does not have correct range information.
        let mut cleanup = CleanupVisitor {
            dropped_lookups: self.dropped_lookups.clone(),
            dropped_features: self.dropped_features.clone(),
            root: new_tree,
            replacement_list: Vec::new(),
        };
        cleanup.visit();
        let new_tree = cleanup.root.edit(cleanup.replacement_list, false);

        let result = new_tree
            .iter_tokens()
            .map(|t| t.as_str())
            .collect::<String>();
        result
    }
}
impl LayoutVisitor for LayoutSubsetVisitor<'_> {
    fn get_root(&self) -> &fea_rs::Node {
        self.parse_tree.root()
    }
    fn depth_first(&self) -> bool {
        false
    }

    // Implement visitor methods here to subset the layout features

    fn visit_gsub1(&mut self, node: &fea_rs::typed::Gsub1) -> bool {
        // Example: Check if the substitution affects glyphs in the glyphset
        #[allow(clippy::unwrap_used)] // We couldn't be here otherwise
        let target = find_first_glyph_or_class(node.node(), None).unwrap();
        #[allow(clippy::unwrap_used)] // We couldn't be here otherwise
        let replacement = find_first_glyph_or_class(node.node(), Some(fea_rs::Kind::ByKw)).unwrap();
        let left_names = glyph_names(&target);
        let mut right_names = glyph_names(&replacement);
        // We're going to rewrite the rule entirely
        let mut new_pairs = vec![];
        if right_names.len() == 1 && left_names.len() > 1 {
            // sub [a b c] by d -> sub [a b c] by [d d d];
            #[allow(clippy::indexing_slicing)] // We just checked lengths
            let single = right_names[0].clone();
            right_names = vec![single; left_names.len()];
        }
        for (left, right) in left_names.iter().zip(right_names.iter()) {
            if self.final_glyphset.contains(&left.as_str())
                && self.final_glyphset.contains(&right.as_str())
            {
                new_pairs.push((left.clone(), right.clone()));
            }
        }
        if new_pairs.is_empty() {
            log::debug!(
                "Dropping GSUB1 substitution not affecting glyphs: {:?} -> {:?}",
                left_names,
                right_names
            );
            self.replacement_list
                .push((node.node().range(), Self::nothing()));
        }
        // Construct a new substitution node if needed
        else if new_pairs.len() != left_names.len() {
            log::debug!(
                "Rewriting GSUB1 substitution to affect glyphs: {:?} -> {:?}",
                left_names,
                right_names
            );
            let mut new_node_text = String::from("sub [");
            for (i, (left, _)) in new_pairs.iter().enumerate() {
                if i > 0 {
                    new_node_text.push(' ');
                }
                new_node_text.push_str(left);
            }
            new_node_text.push_str("] by [");
            for (i, (_, right)) in new_pairs.iter().enumerate() {
                if i > 0 {
                    new_node_text.push(' ');
                }
                new_node_text.push_str(right);
            }
            new_node_text.push_str("];");
            let (new_node, _) = fea_rs::parse::parse_string(new_node_text);
            self.replacement_list
                .push((node.node().range(), new_node.root().clone()));
        } else {
            log::debug!(
                "Keeping GSUB1 substitution affecting glyphs: {:?} -> {:?}",
                left_names,
                right_names
            );
        }
        true
    }

    fn visit_gsub2(&mut self, node: &fea_rs::typed::Gsub2) -> bool {
        #[allow(clippy::unwrap_used)] // We couldn't be here
        let target = find_first_glyph_or_class(node.node(), None).unwrap();
        let involved = std::iter::once(target)
            .chain(
                node.node()
                    .iter_children()
                    .skip_while(|t| t.kind() != fea_rs::Kind::ByKw)
                    .skip(1)
                    .filter_map(GlyphOrClass::cast),
            )
            .flat_map(|g| glyph_names(&g))
            .collect::<IndexSet<_>>();
        if involved
            .iter()
            .all(|g| self.final_glyphset.contains(&g.as_str()))
        {
            log::debug!(
                "Keeping GSUB2 substitution affecting glyphs: {:?}",
                involved
            );
        } else {
            log::debug!(
                "Dropping GSUB2 substitution not affecting glyphs: {:?}",
                involved
            );
            self.replacement_list
                .push((node.node().range(), Self::nothing()));
        }
        true
    }

    fn visit_gsub4(&mut self, node: &fea_rs::typed::Gsub4) -> bool {
        let target = node
            .node()
            .iter_children()
            .take_while(|t| t.kind() != Kind::ByKw)
            .filter_map(GlyphOrClass::cast);
        #[allow(clippy::unwrap_used)] // I mean fea-rs does it so why not
        let glyph: fea_rs::typed::Glyph = node
            .node()
            .iter_children()
            .skip_while(|t| t.kind() != Kind::ByKw)
            .find_map(fea_rs::typed::Glyph::cast)
            .unwrap();
        let involved = target
            .flat_map(|g| glyph_names(&g))
            .chain(match &glyph {
                fea_rs::typed::Glyph::Named(name) => vec![name.text().to_string()],
                _ => vec![],
            })
            .collect::<IndexSet<_>>();
        if involved
            .iter()
            .all(|g| self.final_glyphset.contains(&g.as_str()))
        {
            log::debug!(
                "Keeping GSUB4 substitution affecting glyphs: {:?}",
                involved
            );
        } else {
            log::debug!(
                "Dropping GSUB4 substitution not affecting glyphs: {:?}",
                involved
            );
            self.replacement_list
                .push((node.node().range(), Self::nothing()));
        }
        false
    }

    fn visit_gsub5(&mut self, node: &fea_rs::typed::Gsub5) -> bool {
        true
    }
}

struct CleanupVisitor {
    dropped_lookups: IndexSet<String>,
    dropped_features: IndexSet<String>,
    pub root: fea_rs::Node,
    replacement_list: Vec<(Range<usize>, fea_rs::Node)>,
}
impl CleanupVisitor {
    fn nothing() -> fea_rs::Node {
        let (node, _) = fea_rs::parse::parse_string("");
        node.root().clone()
    }
}
impl LayoutVisitor for CleanupVisitor {
    fn get_root(&self) -> &fea_rs::Node {
        &self.root
    }
    fn depth_first(&self) -> bool {
        true
    }

    fn visit_lookupblock(&mut self, lookup: &LookupBlock) -> bool {
        #[allow(clippy::unwrap_used)] // We've gotta have a name
        let name = lookup
            .node()
            .iter_children()
            .find(|t| t.kind() == Kind::Label)
            .and_then(NodeOrToken::as_token)
            .unwrap()
            .text
            .to_string();
        if self.dropped_lookups.contains(&name) {
            log::debug!("Dropping lookup block '{}'", name);
            self.replacement_list
                .push((lookup.node().range(), Self::nothing()));
            return false;
        }
        // If this block no longer has any effective rules, drop it
        if !lookup
            .node()
            .iter_tokens()
            .any(|child| child.kind == fea_rs::Kind::SubKw || child.kind == fea_rs::Kind::PosKw)
        {
            log::debug!("Dropping empty lookup block '{}'", name);
            self.replacement_list
                .push((lookup.node().range(), Self::nothing()));
            return false;
        }
        true
    }

    fn visit_feature(&mut self, feature: &Feature) -> bool {
        #[allow(clippy::unwrap_used)] // We've gotta have a name
        let name = feature
            .node()
            .iter_children()
            .find(|t| t.kind() == Kind::Tag)
            .and_then(NodeOrToken::as_token)
            .unwrap()
            .text
            .to_string();

        // If this block no longer has any effective rules, drop it
        if !feature
            .node()
            .iter_tokens()
            .any(|child| child.kind == fea_rs::Kind::SubKw || child.kind == fea_rs::Kind::PosKw)
        {
            log::debug!(
                "Dropping empty feature '{}' {:?}",
                name,
                feature.node().range()
            );
            self.replacement_list
                .push((feature.node().range(), Self::nothing()));
            return false;
        }
        true
    }
}
