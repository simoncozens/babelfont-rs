use fea_rs::{
    NodeOrToken, ParseTree,
    typed::{self, AstNode, Gsub1, Gsub2, Gsub3, Gsub4, LookupBlock},
};

#[allow(unused_variables)]
pub trait LayoutVisitor {
    fn get_root(&self) -> ParseTree;
    fn visit_node(&mut self, node: &NodeOrToken) -> bool {
        true
    }
    fn visit_gsub1(&mut self, node: &Gsub1) -> bool {
        true
    }
    fn visit_gsub2(&mut self, node: &Gsub2) -> bool {
        true
    }
    fn visit_gsub3(&mut self, node: &Gsub3) -> bool {
        true
    }
    fn visit_gsub4(&mut self, node: &Gsub4) -> bool {
        true
    }
    fn visit_lookupblock(&mut self, lookup: &LookupBlock) -> bool {
        true
    }
    fn visit(&mut self) {
        let tree = self.get_root();
        let root_node = tree.root();
        self._visit_impl(&NodeOrToken::Node(root_node.clone()));
    }
    fn _visit_impl(&mut self, node: &NodeOrToken) {
        if !self.visit_node(node) {
            return;
        }
        let keep_going = if let Some(node) = typed::Gsub1::cast(node) {
            self.visit_gsub1(&node)
        } else if let Some(node) = typed::Gsub2::cast(node) {
            self.visit_gsub2(&node)
        } else if let Some(node) = typed::Gsub3::cast(node) {
            self.visit_gsub3(&node)
        } else if let Some(node) = typed::Gsub4::cast(node) {
            self.visit_gsub4(&node)
        } else if let Some(lookup) = typed::LookupBlock::cast(node) {
            self.visit_lookupblock(&lookup)
        } else {
            true
        };
        if !keep_going {
            return;
        }
        match node {
            fea_rs::NodeOrToken::Node(node) => {
                for child in node.iter_children() {
                    self._visit_impl(child);
                }
            }
            fea_rs::NodeOrToken::Token(token) => {}
        }
    }
}
