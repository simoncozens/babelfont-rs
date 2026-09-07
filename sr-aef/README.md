# sr-aef

[![crates.io](https://img.shields.io/crates/v/sr-aef.svg)](https://crates.io/crates/sr-aef)
[![docs.rs](https://docs.rs/sr-aef/badge.svg)](https://docs.rs/sr-aef)

`sr-aef` "uncompiles" an OpenType/TrueType font back into Adobe Feature File (`fea`) source code. It reads the OpenType layout tables (GSUB, GPOS and, optionally, GDEF) out of a compiled binary font and reconstructs an equivalent feature file using the [fea-rs-ast] AST.

Because binary fonts lose the original structure of their layout code (lookup splitting, class naming, comments, ...), the output is a faithful *functional* reconstruction rather than a byte-for-byte copy of the original source.

`sr-aef` is used by [babelfont] when compiling a font back to TrueType, and can also be used directly as a "TTF to fea" tool.

## Library usage

The main entry points are [`uncompile`], [`uncompile_bytes`], and [`uncompile_context`].

```rust,no_run
use sr_aef::{fea_rs_ast::AsFea, uncompile_bytes};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data = std::fs::read("MyFont.ttf")?;
    let feature_file = uncompile_bytes(&data, true)?; // `true` also uncompiles GDEF
    println!("{}", feature_file.as_fea(""));
    Ok(())
}
```

The returned value is a [`fea_rs_ast::FeatureFile`]; call `.as_fea()` (via the `AsFea` trait) to render it as feature file text.

For tools that want the component parts rather than a finished feature file, [`uncompile_context`] returns an [`UncompileContext`] with the reconstructed lookups, classes, mark classes, anchors and language systems, ready to be placed wherever you need them.

### Version pinning

`sr-aef` re-exports the exact versions of [fea-rs-ast] and [skrifa] that it is built against (`sr_aef::fea_rs_ast` and `sr_aef::skrifa`). If you are already using those crates, prefer the re-exports so that the AST and font-reading types line up with what `sr-aef` produces.

## Command-line usage

A small CLI is available behind the `cli` feature:

```bash
cargo install sr-aef --features cli
```

Print the reconstructed feature file for a font:

```bash
sr-aef MyFont.ttf
```

Dump the intermediate, partially-uncompiled context as JSON (useful for debugging):

```bash
sr-aef --debug MyFont.ttf
```

## Status

`sr-aef` is in early development and its API may change between releases.

## License

`sr-aef` is available under the MIT or Apache-2.0 licenses, at your option. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).

[babelfont]: https://crates.io/crates/babelfont
[fea-rs-ast]: https://crates.io/crates/fea-rs-ast
[skrifa]: https://crates.io/crates/skrifa
[`uncompile`]: https://docs.rs/sr-aef/latest/sr_aef/fn.uncompile.html
[`uncompile_bytes`]: https://docs.rs/sr-aef/latest/sr_aef/fn.uncompile_bytes.html
[`uncompile_context`]: https://docs.rs/sr-aef/latest/sr_aef/fn.uncompile_context.html
[`UncompileContext`]: https://docs.rs/sr-aef/latest/sr_aef/struct.UncompileContext.html
[`fea_rs_ast::FeatureFile`]: https://docs.rs/fea-rs-ast/latest/fea_rs_ast/struct.FeatureFile.html
