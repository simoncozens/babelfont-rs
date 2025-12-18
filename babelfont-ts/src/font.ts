import { Font as IFont } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import {
  ClassRegistry,
  InflationContext,
  getClassConstructor,
} from "./registry";
import { Axis } from "./axis";
import { Instance } from "./instance";
import { Glyph } from "./glyph";
import { Master } from "./master";
import { Names } from "./names";
import { Features } from "./features";

export interface Font extends WithCamelCase<IFont> {
  names: Names;
  glyphs: Glyph[];
  axes?: Axis[];
  instances?: Instance[];
  masters?: Master[];
  features: Features;
}
export class Font {
  constructor(data: IFont, registry?: ClassRegistry) {
    return InflationContext.with(registry || {}, () => {
      if (data.axes) {
        const AxisClass = getClassConstructor("Axis", Axis);
        data.axes = data.axes.map((a) => new AxisClass(a));
      }
      if (data.instances) {
        const InstanceClass = getClassConstructor("Instance", Instance);
        data.instances = data.instances.map((i) => new InstanceClass(i));
      }
      if (data.glyphs) {
        const GlyphClass = getClassConstructor("Glyph", Glyph);
        data.glyphs = data.glyphs.map((g) => new GlyphClass(g));
      }
      if (data.masters) {
        const MasterClass = getClassConstructor("Master", Master);
        data.masters = data.masters.map((m) => new MasterClass(m));
      }
      if (data.names) {
        const NamesClass = getClassConstructor("Names", Names);
        data.names = new NamesClass(data.names);
      }
      if (data.features) {
        const FeaturesClass = getClassConstructor("Features", Features);
        data.features = new FeaturesClass(data.features);
      }
      Object.assign(this, data);
      return createCaseConvertingProxy(this, Font.prototype) as Font;
    });
  }
}
