use fea_rs_ast::fea_rs;
use fea_rs_ast::fea_rs::parse::ParseTree;
use indexmap::IndexSet;

use crate::layout::visitor::LayoutVisitor;

pub(crate) struct LookupGathererVisitor<'a> {
    parse_tree: &'a ParseTree,
    pub lookup_names: IndexSet<String>,
}

impl<'a> LookupGathererVisitor<'a> {
    pub fn new(parse_tree: &'a ParseTree) -> Self {
        Self {
            parse_tree,
            lookup_names: IndexSet::new(),
        }
    }
}

impl LayoutVisitor for LookupGathererVisitor<'_> {
    fn get_root(&self) -> &fea_rs::Node {
        self.parse_tree.root()
    }

    fn visit_lookupblock(&mut self, lookup: &fea_rs::typed::LookupBlock) -> bool {
        #[allow(clippy::unwrap_used)] // We couldn't be here otherwise
        let node = lookup
            .node()
            .iter_children()
            .find(|n| n.kind() == fea_rs::Kind::Label)
            .unwrap();
        // self.find_token(Kind::Label).unwrap()

        #[allow(clippy::unwrap_used)] // We couldn't be here otherwise
        let name = node.token_text().unwrap().to_string();
        log::debug!("Found lookup block: {}", name);
        self.lookup_names.insert(name);
        false // Save time by stopping there
    }
}
