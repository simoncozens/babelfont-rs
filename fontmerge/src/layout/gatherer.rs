use std::collections::HashSet;

use babelfont::SmolStr;
use fea_rs_ast::{LayoutVisitor, Statement};

/// A layout visitor which collects the name of every lookup and glyph class
/// defined in a feature file, whether at the top level or nested inside a feature
/// or lookup block.
///
/// Glyph class names are collected without their leading `@`, matching the way
/// the subsetter (and the FEA compiler) refer to them.
#[derive(Default)]
pub(crate) struct NameGathererVisitor {
    /// The names of the lookups encountered so far.
    pub lookup_names: HashSet<SmolStr>,
    /// The names of the glyph classes encountered so far, without the leading `@`.
    pub class_names: HashSet<SmolStr>,
}

impl NameGathererVisitor {
    /// Create a new, empty gatherer.
    pub fn new() -> Self {
        Self::default()
    }
}

impl LayoutVisitor for NameGathererVisitor {
    fn visit_statement(&mut self, statement: &mut Statement) -> bool {
        match statement {
            Statement::LookupBlock(lookup_block) => {
                log::debug!("Found lookup block: {}", lookup_block.name);
                self.lookup_names.insert(lookup_block.name.clone());
            }
            Statement::GlyphClassDefinition(class_definition) => {
                log::debug!("Found glyph class: {}", class_definition.name);
                self.class_names
                    .insert(SmolStr::new(&class_definition.name));
            }
            _ => {}
        }
        true
    }
}
