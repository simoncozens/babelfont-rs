import { Layer as ILayer, Path as IPath } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import { getClassConstructor } from "./registry";
import { Guide } from "./guide";
import { Anchor } from "./anchor";
import { Component, Path, isComponent } from "./shape";

export interface Layer extends WithCamelCase<ILayer> {}
export class Layer {
  constructor(data: ILayer) {
    if (data.guides) {
      const GuideClass = getClassConstructor("Guide", Guide);
      data.guides = data.guides.map((g) => new GuideClass(g));
    }
    if (data.shapes) {
      const ComponentClass = getClassConstructor("Component", Component);
      const PathClass = getClassConstructor("Path", Path);
      data.shapes = data.shapes.map((s) =>
        isComponent(s) ? new ComponentClass(s) : new PathClass(s as IPath)
      );
    }
    if (data.anchors) {
      const AnchorClass = getClassConstructor("Anchor", Anchor);
      data.anchors = data.anchors.map((a) => new AnchorClass(a));
    }
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Layer.prototype);
  }
}
