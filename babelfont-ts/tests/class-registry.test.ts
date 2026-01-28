import { Font, Axis, Node, ClassRegistry } from "../src/index";
import { ReviverFunc } from "../src/underlying";
import * as fs from "fs";
import * as path from "path";

const RADIO_CANADA_FONT_PATH = path.join(
  __dirname,
  "../../babelfont/resources/RadioCanadaDisplay.babelfont",
);

describe("Class Registry - User Extensions", () => {
  it("should allow users to extend classes with custom implementations", () => {
    // User creates their own versions of classes with custom methods
    class TestAxis extends Axis {
      getDisplayName(): string {
        return `${this.tag}: ${this.name["dflt"] || ""}`;
      }
    }

    class TestNode extends Node {
      distanceFromOrigin(): number {
        return Math.sqrt(this.x * this.x + this.y * this.y);
      }
    }

    // User creates a registry of their custom classes
    const customRegistry: ClassRegistry = {
      Axis: TestAxis,
      Node: TestNode,
    };

    // Load font with custom registry
    const fileContents = fs.readFileSync(RADIO_CANADA_FONT_PATH, "utf8");
    const rawFont = JSON.parse(fileContents, ReviverFunc);
    const font = new Font(rawFont, customRegistry);

    // Verify axes are instances of TestAxis and have custom methods
    expect(font.axes).toHaveLength(1);
    const axis = font.axes![0];
    expect(axis).toBeInstanceOf(TestAxis);
    // Cast to TestAxis to access custom method
    const testAxis = axis as TestAxis;
    expect(testAxis.getDisplayName()).toBe("wght: Weight");

    // Verify nodes are instances of TestNode and have custom methods
    const glyph = font.glyphs.find((g) => g.name === "A");
    expect(glyph).toBeDefined();
    if (!glyph) return;

    const layer = glyph.layers![0];
    const path1 = layer.shapes?.find((s) => !("reference" in s));
    expect(path1).toBeDefined();
    if (!path1 || "reference" in path1) return;

    const node = path1.nodes?.[0];
    expect(node).toBeInstanceOf(TestNode);
    // Cast to TestNode to access custom method
    const testNode = node as TestNode;
    expect(typeof testNode.distanceFromOrigin()).toBe("number");
    expect(testNode.distanceFromOrigin()).toBeGreaterThanOrEqual(0);
  });

  it("should use default classes when no registry is provided", () => {
    const fileContents = fs.readFileSync(RADIO_CANADA_FONT_PATH, "utf8");
    const rawFont = JSON.parse(fileContents, ReviverFunc);
    const font = new Font(rawFont); // No custom registry

    // Should use default Axis class
    expect(font.axes).toHaveLength(1);
    const axis = font.axes![0];
    expect(axis).toBeInstanceOf(Axis);
    expect(axis.tag).toBe("wght");
  });

  it("should allow partial registry (some custom, some default)", () => {
    class TestAxis extends Axis {
      isVariable(): boolean {
        return true;
      }
    }

    const customRegistry: ClassRegistry = {
      Axis: TestAxis, // Custom
      // Node not specified - will use default
    };

    const fileContents = fs.readFileSync(RADIO_CANADA_FONT_PATH, "utf8");
    const rawFont = JSON.parse(fileContents, ReviverFunc);
    const font = new Font(rawFont, customRegistry);

    // Axis should be custom
    expect(font.axes![0]).toBeInstanceOf(TestAxis);
    expect((font.axes![0] as any).isVariable()).toBe(true);

    // Node should use default
    const glyph = font.glyphs.find((g) => g.name === "A");
    if (glyph?.layers?.[0].shapes?.[0]) {
      const shape = glyph.layers[0].shapes[0];
      if (!("reference" in shape) && shape.nodes?.[0]) {
        expect(shape.nodes[0]).toBeInstanceOf(Node);
      }
    }
  });
});
