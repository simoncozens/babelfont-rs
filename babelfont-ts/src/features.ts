import { Features as IFeatures } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import type { Font } from "./font";
import { WithParent, ensureParentAccessors } from "./parent";

export interface Features extends WithCamelCase<IFeatures>, WithParent<Font> {}
export class Features {
  constructor(data: IFeatures) {
    Object.assign(this, data);
    ensureParentAccessors(this);
    return createCaseConvertingProxy(this, Features.prototype);
  }
}
