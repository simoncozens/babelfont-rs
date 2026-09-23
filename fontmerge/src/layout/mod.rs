//! Layout feature helpers for fontmerge.
//!
//! These visitors are built on `fea_rs_ast`, which is far easier to walk than the
//! lower-level `fea_rs` parse tree (indeed, `fea_rs_ast` exists largely so that we
//! don't have to grub through the parse tree by hand).

pub(crate) mod gatherer;
