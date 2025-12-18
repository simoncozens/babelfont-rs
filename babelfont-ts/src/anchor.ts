import { Anchor as IAnchor } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";

export interface Anchor extends WithCamelCase<IAnchor> {}
export class Anchor {
  constructor(data: IAnchor) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Anchor.prototype);
  }
}
