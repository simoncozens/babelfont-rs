# Glyphs file format v4 - support todo

What follows is the remaining work only. Entries that were already handled, or
that need no code change, have been removed; the "Already covered" section
records why so they are not re-investigated.

## Already covered (no action needed)

* `shape` is now `path | component | image | shapeGroup`; `Shape::Image` and
  `Shape::ShapeGroup` already exist.
* `shapeAttr`, gradients, shadows, the new `lineCap` variants (4/5/6) and
  `layerAttr` (`axisRules`, `color`, `colorPalette` incl. `"*"`, `coordinates`,
  `sbixSize`, `svg`) all live in the `attr` dictionary, which is carried
  verbatim.
* `palettes` (structured `Color Palettes`) is a custom-parameter value, stored
  as a `Plist` and therefore carried through.
* Anchors and guides expose a single `Attributes` dictionary (`attr`) that
  reads both the v4 `attr` key and the earlier `userData` key, and writes `attr`
  for v4 / `userData` for v3. Components keep separate `attr` + `user_data` (v3
  needs both keys) but fold the user data into `attr` when saving v4. Layers
  keep both keys in every version (layerAttr is not user data).
* The following are now typed fields: `kerningContext`,
  `settings.dependencies`, `axis.names`, `axis.userData`, `glyph.axes`,
  `glyph.group`, `glyph.groupIdx`, `instance.id`, `fontMaster.active`,
  `layer.active`, `component.traverseAnchors`, `guide.attr`, `guide.slope`.
* The node type string (tuple item `#2`) now includes the new on-curve types `u`
  (quartic), `h` (Hobby) and `r` (Raph New Spiral) and the `t` tangent
  connection flag, alongside the existing `s` smooth, `R`/`C` orientation and
  `X` locking. `NodeType` models all of these, with `NodeType::as_str` /
  `split_token` centralising the tokens and flag parsing.
* The optional 4th tuple item is a `nodeAttr` object (`{ hoi, userData }`) in v4
  (a bare `userData` dictionary in v3). Deserialization splits it so
  `Node::user_data` behaves the same in both versions and `Node::hoi` holds the
  higher-order interpolation data; serialization recombines them into
  `nodeAttr` for v4 only.
* Package (`.glyphspackage`) reading now picks up `kerning.plist` (merged into
  the top level), `note.md` (the font note) and `features/*.fea` (reconstructed
  as feature prefixes, classes and features, with `.2`/`.3` dedup suffixes
  stripped).

## TODO

_None._ V4 reading support is complete. Package *writing* is not implemented
(currently not wanted).

