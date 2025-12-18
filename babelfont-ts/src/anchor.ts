import { Anchor as IAnchor } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import type { Layer } from "./layer";
import { WithParent, ensureParentAccessors } from "./parent";

export interface Anchor extends WithCamelCase<IAnchor>, WithParent<Layer> {}
export class Anchor {
  constructor(data: IAnchor) {
    Object.assign(this, data);
    ensureParentAccessors(this);
    return createCaseConvertingProxy(this, Anchor.prototype);
  }
}
