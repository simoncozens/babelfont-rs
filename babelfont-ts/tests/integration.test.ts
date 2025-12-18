import {
  Font,
  isComponent,
  Path,
  Node,
  isPath,
  DecomposedAffine,
} from "../src/index";
import { ReviverFunc, ReplacerFunc } from "../src/underlying";
import * as fs from "fs";
import * as path from "path";
import { Component } from "../src/underlying";
import { designspaceToUserspace, Axis as FTAxis } from "@simoncozens/fonttypes";

describe("Babelfont-TS", () => {
  it("should load a .babelfont file and access its properties", () => {
    const babelfontPath = path.join(
      __dirname,
      "../../resources/RadioCanadaDisplay.babelfont"
    );
    const fileContents = fs.readFileSync(babelfontPath, "utf8");
    const rawFont = JSON.parse(fileContents, ReviverFunc);
    const font = new Font(rawFont);

    expect(font.upm).toBe(1000);
    expect(font.version).toEqual([1, 1]);
    expect(font.axes).toHaveLength(1);

    const wghtAxis = font.axes!.find((ax) => ax.tag === "wght");
    expect(wghtAxis).toBeDefined();
    if (!wghtAxis) return; // Keep typescript happy
    expect(wghtAxis.name["dflt"]).toBe("Weight");
    expect(wghtAxis.min).toBe(400);

    expect(font.masters).toHaveLength(2);
    const firstMaster = font.masters![0];
    expect(firstMaster.name["dflt"]).toBe("Regular");

    const wghtLoc = firstMaster!.location!;
    let wghtLocUserspace = designspaceToUserspace(
      wghtLoc,
      font.axes! as unknown as FTAxis[]
    );
    expect(wghtLocUserspace["wght"]).toBe(400);

    expect(font.instances).toHaveLength(4);
    const firstInstance = font.instances![0];
    expect(firstInstance.name["dflt"]).toBe("Regular");

    expect(font.glyphs).toHaveLength(477);

    const capA = font.glyphs.find((g) => g.name === "A");
    expect(isComponent(capA!.layers![0].shapes![0])).toBe(false);

    const aacute = font.glyphs.find((g) => g.name === "Aacute");
    expect(aacute).toBeDefined();
    if (!aacute) return;
    expect(aacute.layers).toHaveLength(4);
    const firstLayer = aacute.layers![0];
    expect(firstLayer.shapes).toHaveLength(2);
    expect(isComponent(firstLayer.shapes![1])).toBe(true);
    const shape = firstLayer.shapes![1] as Component;
    expect(shape.reference).toBe("acutecomb.case");
    expect(shape.transform instanceof DecomposedAffine).toBe(true);

    expect(shape.transform.toAffine()).toEqual([1, 0, 0, 1, 87, 0]);

    expect(font.names.familyName?.["dflt"]).toBe("Radio Canada Display");

    expect(Object.keys(font.firstKernGroups || {}).length).toBeGreaterThan(0);
    expect(Object.keys(font.secondKernGroups || {}).length).toBeGreaterThan(0);
    expect(font.features.features.length).toBeGreaterThan(0);
  });

  it("should serialize a .babelfont file and skip internal properties", () => {
    const babelfontPath = path.join(
      __dirname,
      "../../resources/RadioCanadaDisplay.babelfont"
    );
    const fileContents = fs.readFileSync(babelfontPath, "utf8");
    const rawFont = JSON.parse(fileContents, ReviverFunc);
    const font = new Font(rawFont);
    const serialized = JSON.stringify(font, ReplacerFunc, 2);

    const originalParsed = JSON.parse(fileContents);
    const serializedParsed = JSON.parse(serialized);

    // Verify that keys are preserved in snake_case (not converted to camelCase)
    expect(Object.keys(serializedParsed.names)).toContain("family_name");
    expect(Object.keys(serializedParsed.names)).not.toContain("familyName");
    expect(serializedParsed).toHaveProperty("first_kern_groups");
    expect(serializedParsed).not.toHaveProperty("firstKernGroups");

    // Verify the structure is the same (allowing for date format variations)
    const stripPrecision = (nodes: string): any[] => {
      // Reduce precision of coordinates to
      // avoid floating point serialization differences
      const nodesArr = nodes.split(" ").map((coord) => parseFloat(coord));
      const stripped = nodesArr.map((n) => Math.round(n * 1000) / 1000);
      return stripped;
    };
    const normalize = (obj: any): any => {
      if (obj === null || obj === undefined) return obj;
      if (typeof obj !== "object") return obj;
      if (Array.isArray(obj)) return obj.map(normalize);
      return Object.fromEntries(
        Object.entries(obj)
          .filter(([key]) => key !== "date")
          .map(([key, val]) => [
            key,
            key == "nodes" ? stripPrecision(val as string) : normalize(val),
          ])
      );
    };

    expect(normalize(serializedParsed)).toEqual(normalize(originalParsed));
  });

  it("should be able to navigate around nodes and their parent paths", () => {
    const babelfontPath = path.join(
      __dirname,
      "../../resources/RadioCanadaDisplay.babelfont"
    );
    const fileContents = fs.readFileSync(babelfontPath, "utf8");
    const rawFont = JSON.parse(fileContents, ReviverFunc);
    const font = new Font(rawFont);

    const capO = font.glyphs.find((g) => g.name === "O");
    expect(capO).toBeDefined();
    if (!capO) return;
    const firstLayer = capO.layers![0];
    const firstPath = firstLayer.shapes![0];
    if (!isPath(firstPath)) {
      throw new Error("Expected first shape to be a Path");
    }
    firstPath.formatSpecific = { marker: true };
    expect(firstPath).toBeDefined();
    const firstNode = firstPath.nodes[0];
    expect(firstNode.parent?.formatSpecific!.marker!).toBe(true);
    expect(firstNode.nextNode()).toBeDefined();
    expect(firstNode.previousNode()).toBeDefined();
  });
});
