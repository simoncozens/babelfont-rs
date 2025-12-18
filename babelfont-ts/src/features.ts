import { Features as IFeatures } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";

export interface Features extends WithCamelCase<IFeatures> {}
export class Features {
  constructor(data: IFeatures) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Features.prototype);
  }
}
