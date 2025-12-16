/**
 * Type test file to verify camelCase properties work in IDE
 * This file demonstrates that TypeScript IDE autocomplete works correctly
 */

import { Font, Names, Glyph, Layer, Master, Instance } from "../src/index";

// Test Names interface - both snake_case and camelCase should have autocomplete
const names: Names = {} as any;
const fn1 = names.familyName; // ✓ Should have autocomplete
const fn2 = names.family_name; // ✓ Should also have autocomplete
const ps1 = names.postscriptName; // ✓ camelCase autocomplete
const ps2 = names.postscript_name; // ✓ snake_case autocomplete

// Test Glyph interface - properties and methods
const glyph: Glyph = {} as any;
const pn1 = glyph.productionName; // ✓ Should have autocomplete
const pn2 = glyph.production_name; // ✓ Should also have autocomplete
const layer = glyph.getLayer("id"); // ✓ Method autocomplete should work with JSDoc

// Test Layer interface
const lay: Layer = {} as any;
const li1 = lay.layerIndex; // ✓ camelCase autocomplete
const li2 = lay.layer_index; // ✓ snake_case autocomplete
const bg1 = lay.isBackground; // ✓ camelCase autocomplete
const bg2 = lay.is_background; // ✓ snake_case autocomplete

// Test Master interface
const master: Master = {} as any;
const co1 = master.customOtValues; // ✓ camelCase autocomplete
const co2 = master.custom_ot_values; // ✓ snake_case autocomplete

// Test Instance interface
const instance: Instance = {} as any;
const cn1 = instance.customNames; // ✓ camelCase autocomplete
const cn2 = instance.custom_names; // ✓ snake_case autocomplete

// Test Font - the main interface with nested typed objects
const font: Font = {} as any;

// Font's own properties
const fkg1 = font.firstKernGroups; // ✓ camelCase autocomplete
const fkg2 = font.first_kern_groups; // ✓ snake_case autocomplete
const skg1 = font.secondKernGroups; // ✓ camelCase autocomplete
const skg2 = font.second_kern_groups; // ✓ snake_case autocomplete

// Nested objects should have correct types
const fontNames: Names = font.names; // ✓ Should be Names type, not INames
fontNames.familyName; // ✓ This should work because font.names is properly typed

const fontGlyphs: Glyph[] = font.glyphs; // ✓ Should be Glyph[], not IGlyph[]
if (fontGlyphs.length > 0) {
  const firstGlyph = fontGlyphs[0];
  firstGlyph.productionName; // ✓ Should have autocomplete
  firstGlyph.getLayer("layer-id"); // ✓ Method should be available
}

// Test that array methods preserve types
const glyphsWithProduction = font.glyphs.filter((g) => g.productionName); // ✓ Should work
const glyphLayers = font.glyphs.map((g) => g.layers); // ✓ Should work

// Verify instanceof checks would work at runtime (though this is a type test)
type _AssertNamesIsCorrect = typeof fontNames extends Names ? true : false;
type _AssertGlyphArrayIsCorrect = typeof fontGlyphs extends Glyph[]
  ? true
  : false;
