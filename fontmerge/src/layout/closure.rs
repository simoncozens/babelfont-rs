use babelfont::SmolStr;
use fea_rs::{
    Kind,
    parse::ParseTree,
    typed::{AstNode as _, GlyphOrClass, Gsub1, Gsub2, Gsub3},
};
use fea_rs_ast::fea_rs;
use indexmap::IndexSet;

use crate::layout::{find_first_glyph_or_class, glyph_names, visitor::LayoutVisitor};

pub(crate) struct LayoutClosureVisitor<'a> {
    parse_tree: &'a ParseTree,
    pub glyphset: IndexSet<SmolStr>,
}

impl<'a> LayoutClosureVisitor<'a> {
    pub fn new(parse_tree: &'a ParseTree, glyphset: IndexSet<SmolStr>) -> Self {
        Self {
            parse_tree,
            glyphset,
        }
    }
    fn is_in_glyphset(&self, glyph_or_class: &GlyphOrClass) -> bool {
        glyph_names(glyph_or_class)
            .iter()
            .any(|name| self.glyphset.contains(name))
    }
}

impl LayoutVisitor for LayoutClosureVisitor<'_> {
    fn get_root(&self) -> &fea_rs::Node {
        self.parse_tree.root()
    }

    fn visit_gsub1(&mut self, node: &Gsub1) -> bool {
        #[allow(clippy::unwrap_used)] // We couldn't be here otherwise
        let target = find_first_glyph_or_class(node.node(), None).unwrap();
        let replacement = find_first_glyph_or_class(node.node(), Some(fea_rs::Kind::ByKw));
        if self.is_in_glyphset(&target)
            && let Some(r) = replacement
        {
            log::debug!(
                "Adding glyphs from GSUB substitution: {:?} -> {:?}",
                target,
                r
            );
            self.glyphset.extend(glyph_names(&r));
        }
        true
    }

    fn visit_gsub2(&mut self, node: &Gsub2) -> bool {
        #[allow(clippy::unwrap_used)] // We couldn't be here otherwise
        let target = find_first_glyph_or_class(node.node(), None).unwrap();
        if self.is_in_glyphset(&target) {
            for r in node
                .node()
                .iter_children()
                .skip_while(|t| t.kind() != Kind::ByKw)
                .skip(1)
                .filter_map(GlyphOrClass::cast)
            {
                log::debug!(
                    "Adding glyphs from GSUB substitution: {:?} -> {:?}",
                    target,
                    r
                );
                self.glyphset.extend(glyph_names(&r));
            }
        }
        true
    }

    fn visit_gsub3(&mut self, node: &Gsub3) -> bool {
        #[allow(clippy::unwrap_used)] // We couldn't be here otherwise
        let target = node
            .node()
            .iter_children()
            .find(|c| c.kind() == fea_rs::Kind::GlyphName)
            .unwrap();
        #[allow(clippy::unwrap_used)] // We couldn't be here otherwise
        let alternates = fea_rs::typed::GlyphClass::cast(
            node.node()
                .iter_children()
                .skip_while(|t| t.kind() != Kind::FromKw)
                .find(|c| c.kind() == fea_rs::Kind::GlyphClass)
                .unwrap(),
        )
        .unwrap();
        #[allow(clippy::unwrap_used)] // OK, I *think*
        let glyph_name = target.token_text().unwrap();
        if self.glyphset.contains(glyph_name) {
            match alternates {
                fea_rs::typed::GlyphClass::Named(_glyph_class_name) => {
                    // XXX resolve glyph class to glyph names
                }
                fea_rs::typed::GlyphClass::Literal(glyph_class_literal) => {
                    log::debug!(
                        "Adding glyphs from GSUB substitution: {:?} -> {:?}",
                        target,
                        glyph_class_literal
                    );
                    for item in glyph_class_literal
                        .iter()
                        .skip_while(|t| t.kind() != Kind::LSquare)
                        .skip(1)
                        .take_while(|t| t.kind() != Kind::RSquare)
                        // .filter(|t| !t.kind().is_trivia())
                        .flat_map(|i| i.token_text())
                    {
                        self.glyphset.insert(item.into());
                    }
                }
            }
        }

        true
    }

    fn visit_gsub4(&mut self, node: &fea_rs::typed::Gsub4) -> bool {
        let mut target = node
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
        if let fea_rs::typed::Glyph::Named(glyph_name) = glyph
            && target.all(|t| self.is_in_glyphset(&t))
        {
            let name: SmolStr = glyph_name.text().clone();
            if !self.glyphset.contains(&name) {
                log::debug!(
                    "Adding glyph '{}' by closing over GSUB substitution: {}",
                    name,
                    node.node()
                        .iter_tokens()
                        .map(|t| t.as_str())
                        .collect::<String>(),
                );
                self.glyphset.insert(name);
            }
        }
        true
    }
}
