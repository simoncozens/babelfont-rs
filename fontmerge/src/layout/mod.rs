use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use babelfont::SmolStr;
use fea_rs_ast::{
    fea_rs,
    fea_rs::{
        GlyphMap, Kind, ParseTree,
        compile::NopVariationInfo,
        parse::{FileSystemResolver, SourceResolver},
        typed::{AstNode, GlyphOrClass},
    },
};
use itertools::Either;

use crate::error::FontmergeError;

pub(crate) mod closure;
pub(crate) mod lookupgatherer;
pub(crate) mod visitor;

pub(crate) fn get_parse_tree(
    features: &str,
    glyph_names: &[&SmolStr],
    project_root: impl Into<PathBuf>,
) -> Result<ParseTree, FontmergeError> {
    let glyph_map = glyph_names.iter().cloned().collect::<GlyphMap>();
    let resolver: FileSystemResolver = FileSystemResolver::new(project_root.into());
    let features_text: Arc<str> = Arc::from(features);
    let (parse_tree, diagnostics) = fea_rs::parse::parse_root(
        "get_parse_tree".into(),
        Some(&glyph_map),
        Box::new(move |s: &Path| {
            if s == Path::new("get_parse_tree") {
                Ok(features_text.clone())
            } else {
                resolver.get_contents(s)
            }
        }),
    )?;
    if diagnostics.has_errors() {
        log::error!("Errors encountered while parsing feature file for layout closure:");
        log::error!("{}", diagnostics.display());
        return Err(FontmergeError::LayoutClosureError);
    }

    let diagnostics = fea_rs::compile::validate::<NopVariationInfo>(&parse_tree, &glyph_map, None);
    if diagnostics.has_errors() {
        log::error!("Errors encountered while validating feature file for layout closure:");
        log::error!("{}", diagnostics.display());
        return Err(FontmergeError::LayoutClosureError);
    }
    Ok(parse_tree)
}

pub(crate) fn find_first_glyph_or_class(
    node: &fea_rs::Node,
    after: Option<Kind>,
) -> Option<GlyphOrClass> {
    let iter = if let Some(kind) = after {
        Either::Left(
            node.iter_children()
                .skip_while(move |c| c.kind() != kind)
                .skip(1),
        )
    } else {
        Either::Right(node.iter_children())
    };
    for child in iter {
        match child.kind() {
            fea_rs::Kind::GlyphClass => {
                if let Some(gc) = GlyphOrClass::cast(child) {
                    return Some(gc);
                }
            }
            fea_rs::Kind::GlyphName => {
                if let Some(g) = GlyphOrClass::cast(child) {
                    return Some(g);
                }
            }
            // One day handle literal glyph classes
            _ => {}
        }
    }
    None
}

pub(crate) fn glyph_names(gc: &GlyphOrClass) -> Vec<SmolStr> {
    match gc {
        GlyphOrClass::Glyph(name) => vec![name.text().to_string().into()],
        GlyphOrClass::Class(names) => names
            .iter()
            .filter_map(|n| {
                if n.is_glyph_or_glyph_class() {
                    n.token_text().map(|t| t.to_string().into())
                } else {
                    None
                }
            })
            .collect(),
        _ => vec![],
    }
}
