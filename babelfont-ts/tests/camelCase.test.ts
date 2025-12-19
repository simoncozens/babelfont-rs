import { Font, Names } from "../src/index";
import { ReviverFunc } from "../src/underlying";
import * as fs from "fs";
import * as path from "path";

describe("CamelCase Property Access", () => {
  it("should allow camelCase access to snake_case properties", () => {
    const babelfontPath = path.join(
      __dirname,
      "../../resources/RadioCanadaDisplay.babelfont",
    );
    const fileContents = fs.readFileSync(babelfontPath, "utf8");
    const rawFont = JSON.parse(fileContents, ReviverFunc);
    const font = new Font(rawFont);

    // Test that camelCase property access works
    expect(font.names.familyName?.["dflt"]).toBe("Radio Canada Display");

    // Also test that snake_case still works
    expect(font.names.family_name?.["dflt"]).toBe("Radio Canada Display");

    // Test more camelCase conversions
    expect(font.firstKernGroups).toBeDefined();
    expect(font.secondKernGroups).toBeDefined();

    // Also verify snake_case versions work
    expect(font.first_kern_groups).toBeDefined();
    expect(font.second_kern_groups).toBeDefined();

    // Test on nested objects
    const firstGlyph = font.glyphs.find((g) => g.productionName);
    if (firstGlyph && firstGlyph.productionName) {
      // Verify both camelCase and snake_case work
      expect(firstGlyph.productionName).toBe(firstGlyph.production_name);
    }
  });

  it("should work with methods on proxied objects", () => {
    const babelfontPath = path.join(
      __dirname,
      "../../resources/RadioCanadaDisplay.babelfont",
    );
    const fileContents = fs.readFileSync(babelfontPath, "utf8");
    const rawFont = JSON.parse(fileContents, ReviverFunc);
    const font = new Font(rawFont);

    const aacute = font.glyphs.find((g) => g.name === "Aacute");
    expect(aacute).toBeDefined();
    if (!aacute) return;

    // Test that methods still work on proxied objects
    const firstLayer = aacute.layers![0];
    const foundLayer = aacute.getLayer(firstLayer.id!);
    expect(foundLayer).toBe(firstLayer);
  });

  it("should allow setting properties via camelCase", () => {
    const names = new Names({
      family_name: { dflt: "Test Family" },
    });

    // Read via camelCase
    expect(names.familyName?.["dflt"]).toBe("Test Family");

    // Set via camelCase
    names.familyName = { dflt: "New Family" };

    // Verify it updated the underlying snake_case property
    expect(names.family_name?.["dflt"]).toBe("New Family");
  });
});
