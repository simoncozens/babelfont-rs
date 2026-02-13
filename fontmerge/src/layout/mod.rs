use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use babelfont::SmolStr;
use fea_rs_ast::{
    fea_rs,
    fea_rs::{
        GlyphMap, ParseTree,
        compile::NopVariationInfo,
        parse::{FileSystemResolver, SourceResolver},
    },
};

use crate::error::FontmergeError;

// pub(crate) mod lookupgatherer;
// pub(crate) mod visitor;
