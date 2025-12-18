export type {
  Direction,
  LayerType,
  GlyphCategory,
  NodeType,
  MetricType,
  StyleMapStyle,
  I18NDictionary,
  Shape,
  Path as IPath,
  Node as INode,
  Component as IComponent,
  DecomposedAffine as IDecomposedAffine,
  Anchor as IAnchor,
  Axis as IAxis,
  Color as IColor,
  Features as IFeatures,
  Instance as IInstance,
  Names as INames,
  Position as IPosition,
  Guide as IGuide,
  OTValue as IOTValue,
  Master as IMaster,
  Layer as ILayer,
  Glyph as IGlyph,
  Font as IFont,
} from "./underlying";

export type { ClassRegistry } from "./registry";
export { InflationContext, getClassConstructor } from "./registry";
export { Anchor } from "./anchor";
export { Axis } from "./axis";
export { Color } from "./color";
export { Component, Node, Path, isComponent, isPath } from "./shape";
export { DecomposedAffine } from "./decomposedAffine";
export { Features } from "./features";
export { Names } from "./names";
export { Instance } from "./instance";
export { Position } from "./position";
export { Guide } from "./guide";
export { OTValue } from "./otvalue";
export { Master } from "./master";
export { Layer } from "./layer";
export { Glyph } from "./glyph";
export { Font } from "./font";
