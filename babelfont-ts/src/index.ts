import {
  Anchor as IAnchor,
  Axis as IAxis,
  Color as IColor,
  Component as IComponent,
  Features as IFeatures,
  Instance as IInstance,
  Names as INames,
  Position as IPosition,
  Guide as IGuide,
  OTValue as IOTValue,
  Master as IMaster,
  Node as INode,
  Path as IPath,
  Layer as ILayer,
  Shape,
  Direction,
  LayerType,
  GlyphCategory,
  Glyph as IGlyph,
  Font as IFont,
  NodeType,
  MetricType,
  StyleMapStyle,
  I18NDictionary,
} from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";

export type {
  Direction,
  LayerType,
  GlyphCategory,
  NodeType,
  MetricType,
  StyleMapStyle,
  I18NDictionary,
};

// This is a trick to avoid duplicating the fields of the interfaces in the classes.
// https://stackoverflow.com/questions/65787571/avoiding-fields-declaration-duplication-in-interface-and-class-definition
export interface Anchor extends WithCamelCase<IAnchor> {}
export class Anchor {
  constructor(data: IAnchor) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Anchor.prototype);
  }
}

export interface Axis extends WithCamelCase<IAxis> {}
export class Axis {
  constructor(data: IAxis) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Axis.prototype);
  }
}

export interface Color extends WithCamelCase<IColor> {}
export class Color {
  constructor(data: IColor) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Color.prototype);
  }
}
export interface Component extends WithCamelCase<IComponent> {}
export class Component {
  constructor(data: IComponent) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Component.prototype);
  }
}

export interface Features extends WithCamelCase<IFeatures> {}
export class Features {
  constructor(data: IFeatures) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Features.prototype);
  }
}

export interface Names extends WithCamelCase<INames> {}
export class Names {
  constructor(data: INames) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Names.prototype) as Names;
  }
}

export interface Instance extends WithCamelCase<IInstance> {}
export class Instance {
  constructor(data: IInstance) {
    // inflate the .custom_names property
    if (data.custom_names) {
      data.custom_names = new Names(data.custom_names);
    }
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Instance.prototype);
  }
}
export interface Position extends WithCamelCase<IPosition> {}
export class Position {
  constructor(data: IPosition) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Position.prototype);
  }
}

export interface Guide extends WithCamelCase<IGuide> {}
export class Guide {
  constructor(data: IGuide) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Guide.prototype);
  }
}

export interface OTValue extends WithCamelCase<IOTValue> {}
export class OTValue {
  constructor(data: IOTValue) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, OTValue.prototype);
  }
}

export interface Master extends WithCamelCase<IMaster> {
  customOtValues?: OTValue[];
}
export class Master {
  constructor(data: IMaster) {
    // Inflate the .guides array
    if (data.guides) {
      data.guides = data.guides.map((g) => new Guide(g));
    }
    // Infate .custom_ot_values
    if (data.custom_ot_values) {
      data.custom_ot_values = data.custom_ot_values.map(
        (otv) => new OTValue(otv)
      );
    }
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Master.prototype);
  }
}

export interface Node extends WithCamelCase<INode> {
  __parent?: Path;
  nextNode(): Node | undefined;
  previousNode(): Node | undefined;
  nextOnCurveNode(): Node | undefined;
  previousOnCurveNode(): Node | undefined;
}
export class Node {
  constructor(data: INode) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Node.prototype);
  }
  /** The parent Path containing this Node */
  get parent(): Path | undefined {
    return this.__parent;
  }
  set parent(value: Path | undefined) {
    this.__parent = value;
  }

  nextNode(): Node | undefined {
    if (!this.parent) return undefined;
    const index = this.parent.nodes.indexOf(this);
    // Wrap around
    const nextIndex = (index + 1) % this.parent.nodes.length;
    return this.parent.nodes[nextIndex] as Node;
  }

  previousNode(): Node | undefined {
    if (!this.parent) return undefined;
    const index = this.parent.nodes.indexOf(this);
    // Wrap around
    const prevIndex =
      (index - 1 + this.parent.nodes.length) % this.parent.nodes.length;
    return this.parent.nodes[prevIndex] as Node;
  }

  nextOnCurveNode(): Node | undefined {
    let currentNode: Node | undefined = this;
    do {
      currentNode = currentNode?.nextNode();
      if (currentNode?.nodetype != NodeType.OffCurve) {
        return currentNode;
      }
    } while (currentNode && currentNode !== this);
    return undefined;
  }

  previousOnCurveNode(): Node | undefined {
    let currentNode: Node | undefined = this;
    do {
      currentNode = currentNode?.previousNode();
      if (currentNode?.nodetype != NodeType.OffCurve) {
        return currentNode;
      }
    } while (currentNode && currentNode !== this);
    return undefined;
  }

  toJSON(): INode {
    // Exclude the __parent property when serializing to JSON
    const { __parent, ...rest } = this;
    return rest;
  }
}

export interface Path extends WithCamelCase<IPath> {
  nodes: Node[];
}
export class Path {
  constructor(data: IPath) {
    // Inflate the .nodes array
    if (data.nodes) {
      // For brevity the nodes are stored as a string, but we need to inflate them
      data.nodes = Path.parseNodes(data.nodes as unknown as string).map((n) => {
        let nObject = new Node(n);
        nObject.parent = this;
        return createCaseConvertingProxy(nObject, Node.prototype) as Node;
      });
    }
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Path.prototype);
  }

  private static parseNodes(nodes: string): Node[] {
    const nodesStr = nodes.trim();
    if (!nodesStr) return [];

    const tokens: string[] = nodesStr.split(/\s+/);
    const nodesArray: Node[] = [];

    for (let i = 0; i + 2 < tokens.length; i += 3) {
      let nodetype = NodeType.Line;
      let smooth = false;
      switch (tokens[i + 2]!) {
        case "m":
          nodetype = NodeType.Move;
          break;
        case "l":
          nodetype = NodeType.Line;
          break;
        case "ls":
          nodetype = NodeType.Line;
          smooth = true;
          break;
        case "o":
          nodetype = NodeType.OffCurve;
          break;
        case "c":
          nodetype = NodeType.Curve;
          break;
        case "cs":
          nodetype = NodeType.Curve;
          smooth = true;
          break;
        case "q":
          nodetype = NodeType.QCurve;
          break;
        case "qs":
          nodetype = NodeType.QCurve;
          smooth = true;
          break;
      }
      nodesArray.push(
        new Node({
          x: parseFloat(tokens[i]!), // x
          y: parseFloat(tokens[i + 1]!), // y
          nodetype,
          smooth,
        })
      );
    }
    return nodesArray;
  }

  toJSON(): IPath {
    // Serialize the .nodes array back to the string format
    const nodesStr = this.nodes
      .map((n) => {
        let typeChar = "l";
        switch (n.nodetype) {
          case NodeType.Move:
            typeChar = "m";
            break;
          case NodeType.Line:
            typeChar = "l";
            break;
          case NodeType.OffCurve:
            typeChar = "o";
            break;
          case NodeType.Curve:
            typeChar = "c";
            break;
          case NodeType.QCurve:
            typeChar = "q";
            break;
        }
        let smoothChar = n.smooth ? "s" : "";
        return `${n.x} ${n.y} ${typeChar}${smoothChar}`;
      })
      .join(" ");
    // Exclude the nodes property and replace with the string version
    const { nodes, ...rest } = this;
    return {
      ...rest,
      nodes: nodesStr,
    } as unknown as IPath;
  }
}

export function isComponent(shape: Shape): shape is Component {
  return (shape as Component).reference !== undefined;
}
export function isPath(shape: Shape): shape is Path {
  return (shape as Path).nodes !== undefined;
}

export interface Layer extends WithCamelCase<ILayer> {}
export class Layer {
  constructor(data: ILayer) {
    if (data.guides) {
      data.guides = data.guides.map((g) => new Guide(g));
    }
    // Inflate the .shapes array
    if (data.shapes) {
      data.shapes = data.shapes.map((s) =>
        isComponent(s) ? new Component(s) : new Path(s as IPath)
      );
    }
    // .anchors
    if (data.anchors) {
      data.anchors = data.anchors.map((a) => new Anchor(a));
    }
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Layer.prototype);
  }
}

export interface Glyph extends WithCamelCase<IGlyph> {
  layers: Layer[];
}
export class Glyph {
  constructor(data: IGlyph) {
    // Inflate the .layers array
    if (data.layers) {
      data.layers = data.layers.map((l) => {
        return createCaseConvertingProxy(
          new Layer(l),
          Layer.prototype
        ) as Layer;
      });
    }
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Glyph.prototype) as Glyph;
  }

  /**
   * Get a layer by its ID
   * @param layerId - The ID of the layer to find
   * @returns The layer with the given ID, or undefined if not found
   */
  getLayer(layerId: string): Layer | undefined {
    return this.layers?.find((layer) => layer.id === layerId);
  }
}

export interface Font extends WithCamelCase<IFont> {
  // Override with our proxied types
  names: Names;
  glyphs: Glyph[];
  axes?: Axis[];
  instances?: Instance[];
  masters?: Master[];
  features: Features;
}
export class Font {
  constructor(data: IFont) {
    // Inflate the .axes array
    if (data.axes) {
      data.axes = data.axes.map((a) => new Axis(a));
    }
    // .instances
    if (data.instances) {
      data.instances = data.instances.map((i) => new Instance(i));
    }
    // .glyphs
    if (data.glyphs) {
      data.glyphs = data.glyphs.map((g) => new Glyph(g));
    }
    // .masters
    if (data.masters) {
      data.masters = data.masters.map((m) => new Master(m));
    }
    // .names
    if (data.names) {
      data.names = new Names(data.names);
    }
    // .features
    if (data.features) {
      data.features = new Features(data.features);
    }
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Font.prototype) as Font;
  }
}
