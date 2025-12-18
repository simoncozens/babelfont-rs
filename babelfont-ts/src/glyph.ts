import { Glyph as IGlyph } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import { getClassConstructor } from "./registry";
import { Layer } from "./layer";
import type { Font } from "./font";
import { WithParent, ensureParentAccessors, setParent } from "./parent";

export interface Glyph extends WithCamelCase<IGlyph>, WithParent<Font> {
  layers: Layer[];
}
export class Glyph {
  constructor(data: IGlyph) {
    ensureParentAccessors(this);
    if (data.layers) {
      const LayerClass = getClassConstructor("Layer", Layer);
      data.layers = data.layers.map((l) => {
        const layer = new LayerClass(l) as Layer;
        return layer;
      });
    }
    Object.assign(this, data);
    const proxied = createCaseConvertingProxy(this, Glyph.prototype) as Glyph;
    proxied.layers?.forEach((layer) =>
      setParent(layer as unknown as WithParent<Glyph>, proxied)
    );
    return proxied;
  }

  /**
   * Get a layer by its ID.
   * @param layerId
   * @returns The layer with the specified ID, or undefined if not found.
   */
  getLayer(layerId: string): Layer | undefined {
    return this.layers?.find((layer) => layer.id === layerId);
  }
}
