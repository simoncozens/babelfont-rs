use fea_rs::{
    Node, NodeOrToken,
    typed::{self, AstNode, Feature, Gsub1, Gsub2, Gsub3, Gsub4, Gsub5, Gsub6, LookupBlock},
};

// Use constants as documentation
pub(crate) const STOP: bool = false;
pub(crate) const CONTINUE: bool = true;

#[allow(unused_variables)]
pub trait LayoutVisitor {
    fn get_root(&self) -> &Node;
    fn depth_first(&self) -> bool {
        true
    }
    fn visit_node(&mut self, node: &NodeOrToken) -> bool {
        CONTINUE
    }
    fn visit_gsub1(&mut self, node: &Gsub1) -> bool {
        CONTINUE
    }
    fn visit_gsub2(&mut self, node: &Gsub2) -> bool {
        CONTINUE
    }
    fn visit_gsub3(&mut self, node: &Gsub3) -> bool {
        CONTINUE
    }
    fn visit_gsub4(&mut self, node: &Gsub4) -> bool {
        CONTINUE
    }
    fn visit_gsub5(&mut self, node: &Gsub5) -> bool {
        CONTINUE
    }
    fn visit_gsub6(&mut self, node: &Gsub6) -> bool {
        CONTINUE
    }
    fn visit_lookupblock(&mut self, lookup: &LookupBlock) -> bool {
        CONTINUE
    }
    fn visit_feature(&mut self, features: &Feature) -> bool {
        CONTINUE
    }
    fn visit(&mut self) {
        let root_node = self.get_root();
        self._visit_impl(&NodeOrToken::Node(root_node.clone()));
    }
    fn _visit_impl(&mut self, node: &NodeOrToken) {
        // Pre-order visit: visit the node and return if we're told to stop
        if !self.depth_first() && self.visit_node(node) == STOP {
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
        } else if let Some(node) = typed::Gsub5::cast(node) {
            self.visit_gsub5(&node)
        } else if let Some(node) = typed::Gsub6::cast(node) {
            self.visit_gsub6(&node)
        } else if let Some(lookup) = typed::LookupBlock::cast(node) {
            self.visit_lookupblock(&lookup)
        } else if let Some(features) = typed::Feature::cast(node) {
            self.visit_feature(&features)
        } else {
            CONTINUE
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

        // Post-order visit: now we've visited the children, visit the node
        if self.depth_first() {
            self.visit_node(node);
        }
    }
}
