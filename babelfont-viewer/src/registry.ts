import { Axis, Node, Path } from "babelfont";
import type { ClassRegistry } from "babelfont";

export class MyAxis extends Axis {
  getDisplayName(): string {
    const name = this.name?.["dflt"] || Object.values(this.name || {})[0] || "";
    return name.trim();
  }
  getRange(): number {
    const min = (this.min as number) || 0;
    const max = (this.max as number) || 0;
    return max - min;
  }
}

export class MyNode extends Node {
  distanceFromOrigin(): number {
    return Math.sqrt(this.x * this.x + this.y * this.y);
  }
  moveTo(x: number, y: number): void {
    this.x = x;
    this.y = y;
  }
}

export class MyPath extends Path {
  /**
   * Convert path nodes to an SVG path string.
   * M first-node, then L for lines and C for cubic curves using preceding off-curve points; finally Z.
   */
  toSvgPathString(): string {
    const nodes = this.nodes || [];
    if (nodes.length === 0) return "";
    const parts: string[] = [];
    const first = nodes[0];
    parts.push(`M ${first.x} ${first.y}`);
    const offcurves: Array<[number, number]> = [];
    for (let i = 1; i < nodes.length; i++) {
      const n = nodes[i];
      const t = n.nodetype;
      if (t === "OffCurve") {
        offcurves.push([n.x, n.y]);
      } else if (t === "Line") {
        parts.push(`L ${n.x} ${n.y}`);
        offcurves.length = 0;
      } else if (t === "Curve") {
        const c1 =
          offcurves.length >= 2 ? offcurves[offcurves.length - 2] : undefined;
        const c2 =
          offcurves.length >= 1 ? offcurves[offcurves.length - 1] : undefined;
        if (c1 && c2) {
          parts.push(`C ${c1[0]} ${c1[1]} ${c2[0]} ${c2[1]} ${n.x} ${n.y}`);
        } else {
          // Fallback if control points missing: treat as line
          parts.push(`L ${n.x} ${n.y}`);
        }
        offcurves.length = 0;
      }
    }
    parts.push("Z");
    return parts.join(" ");
  }
}

export const customRegistry: ClassRegistry = {
  Axis: MyAxis,
  Node: MyNode,
  Path: MyPath,
};
