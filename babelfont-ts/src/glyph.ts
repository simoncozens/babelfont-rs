import { Glyph as IGlyph } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import { getClassConstructor } from "./registry";
import { Layer } from "./layer";

export interface Glyph extends WithCamelCase<IGlyph> {
  layers: Layer[];
}
export class Glyph {
  constructor(data: IGlyph) {
    if (data.layers) {
      const LayerClass = getClassConstructor("Layer", Layer);
      data.layers = data.layers.map((l) => {
        return createCaseConvertingProxy(
          new LayerClass(l),
          LayerClass.prototype
        ) as Layer;
      });
    }
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Glyph.prototype) as Glyph;
  }

  getLayer(layerId: string): Layer | undefined {
    return this.layers?.find((layer) => layer.id === layerId);
  }
}
