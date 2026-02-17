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
import { WithParent, ensureParentAccessors, setParent } from "./parent";

export interface Font extends WithCamelCase<IFont>, WithParent<Font> {
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
      ensureParentAccessors(this);

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
      const proxied = createCaseConvertingProxy(this, Font.prototype) as Font;

      proxied.axes?.forEach((axis) =>
        setParent((axis as unknown) as WithParent<Font>, proxied)
      );
      proxied.instances?.forEach((inst) =>
        setParent((inst as unknown) as WithParent<Font>, proxied)
      );
      proxied.glyphs?.forEach((glyph) =>
        setParent((glyph as unknown) as WithParent<Font>, proxied)
      );
      proxied.masters?.forEach((master) =>
        setParent((master as unknown) as WithParent<Font>, proxied)
      );
      if (proxied.names)
        setParent((proxied.names as unknown) as WithParent<Font>, proxied);
      if (proxied.features)
        setParent((proxied.features as unknown) as WithParent<Font>, proxied);

      return proxied;
    });
  }
}
