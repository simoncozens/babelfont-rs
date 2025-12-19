import { Layer as ILayer, Path as IPath } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import { getClassConstructor } from "./registry";
import { Guide } from "./guide";
import { Anchor } from "./anchor";
import { Component, Path, isComponent } from "./shape";
import { WithParent, ensureParentAccessors, setParent } from "./parent";
import type { Glyph } from "./glyph";

export interface Layer extends WithCamelCase<ILayer>, WithParent<Glyph> {
  shapes: (Component | Path)[];
}
export class Layer {
  constructor(data: ILayer) {
    ensureParentAccessors(this);
    if (data.guides) {
      const GuideClass = getClassConstructor("Guide", Guide);
      data.guides = data.guides.map((g) => new GuideClass(g));
    }
    if (data.shapes) {
      const ComponentClass = getClassConstructor("Component", Component);
      const PathClass = getClassConstructor("Path", Path);
      data.shapes = data.shapes.map((s) =>
        isComponent(s)
          ? (new ComponentClass(s) as Component)
          : (new PathClass(s as IPath) as Path),
      );
    }
    if (data.anchors) {
      const AnchorClass = getClassConstructor("Anchor", Anchor);
      data.anchors = data.anchors.map((a) => new AnchorClass(a));
    }
    Object.assign(this, data);
    const proxied = createCaseConvertingProxy(this, Layer.prototype) as Layer;
    proxied.guides?.forEach((guide) =>
      setParent(guide as unknown as WithParent<Layer>, proxied),
    );
    proxied.shapes?.forEach((shape) =>
      setParent(shape as unknown as WithParent<Layer>, proxied),
    );
    proxied.anchors?.forEach((anchor) =>
      setParent(anchor as unknown as WithParent<Layer>, proxied),
    );
    return proxied;
  }
}
