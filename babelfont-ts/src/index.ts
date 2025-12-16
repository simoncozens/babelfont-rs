import type {
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

export interface Node extends WithCamelCase<INode> {}
export class Node {
  constructor(data: INode) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Node.prototype);
  }
}

export interface Path extends WithCamelCase<IPath> {}
export class Path {
  constructor(data: IPath) {
    // Inflate the .nodes array
    if (data.nodes && Array.isArray(data.nodes)) {
      data.nodes = data.nodes.map((n) => new Node(n));
    }
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Path.prototype);
  }
}

export function isComponent(shape: Shape): shape is IComponent {
  return (shape as IComponent).reference !== undefined;
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
        isComponent(s) ? new Component(s) : new Path(s)
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
      data.layers = data.layers.map((l) => new Layer(l));
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
