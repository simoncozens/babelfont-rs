# fontmerge

[![crates.io](https://img.shields.io/crates/v/fontmerge.svg)](https://crates.io/crates/fontmerge)
[![docs.rs](https://docs.rs/fontmerge/badge.svg)](https://docs.rs/fontmerge)

`fontmerge` merges two font source files together, copying selected glyphs from a *donor* font into a *host* font, along with any necessary OpenType layout features. It is a font-merging toolkit that powers the `fontmerge` command-line tool.

Typical use cases include:

- Adding a set of glyphs (e.g. a Latin or Cyrillic extension) to an existing font.
- Copying glyphs selected by name or by Unicode codepoint from one source into another.
- Transporting OpenType layout rules (features, lookups, mark classes) alongside the glyphs they reference.

Merging is performed on font *sources* (Glyphs, UFO/DesignSpace, Babelfont JSON, ...) via the [babelfont] crate rather than on compiled binaries, so you keep full editability after the merge.

## Command-line usage

Install the CLI with:

```bash
cargo install fontmerge
```

Merge all of `Donor.glyphs` into `Host.glyphs`:

```bash
fontmerge Host.glyphs Donor.glyphs --output Merged.glyphs
```

Copy only specific glyphs by name:

```bash
fontmerge Host.glyphs Donor.glyphs --output Merged.glyphs --glyphs "A B C D"
```

Copy glyphs by Unicode codepoint (comma-separated, `U+` prefix optional, ranges with a hyphen allowed):

```bash
fontmerge Host.glyphs Donor.glyphs --output Merged.glyphs --codepoints "U+0041-U+005A"
```

Exclude glyphs you don't want to bring across:

```bash
fontmerge Host.glyphs Donor.glyphs --output Merged.glyphs --glyphs "A B C" --exclude-glyphs "Aacute B"
```

By default, glyphs already present in the host font are skipped; use `--existing-handling replace` to overwrite them instead. Layout rules can be subset to the merged glyph set (default), closed over to pull in any extra glyphs referenced by rules (`--layout-handling closure`), or ignored entirely (`--layout-handling ignore`).

## Using it as a library

The `fontmerge` crate is a mixed binary/library crate. Its library exposes the underlying merging routine so you can drive merges programmatically:

```rust,no_run
use babelfont::{load, SmolStr};
use fontmerge::{fontmerge, ExistingGlyphHandling, GlyphsetFilter, LayoutHandling};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // "Host" font receives glyphs; "donor" font contributes them.
    let mut host = load("Host.glyphs")?;
    let donor = load("Donor.glyphs")?;

    // Select which glyphs to copy from the donor font.
    let selection = GlyphsetFilter::new(
        vec![SmolStr::new("A"), SmolStr::new("B")],
        vec![],
        vec![],
        &mut host,
        &donor,
        ExistingGlyphHandling::Skip,
    );

    let merged = fontmerge(host, donor, selection, LayoutHandling::Subset, true)?;
    merged.save("Merged.glyphs")?;
    Ok(())
}
```

See the [API documentation](https://docs.rs/fontmerge) for details.

## Status

`fontmerge` is in early development and its command-line interface and library API may change between releases.

## License

`fontmerge` is available under the MIT or Apache-2.0 licenses, at your option. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).

[babelfont]: https://crates.io/crates/babelfont
