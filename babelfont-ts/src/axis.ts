import { Axis as IAxis } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";

export interface Axis extends WithCamelCase<IAxis> {}
export class Axis {
  constructor(data: IAxis) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Axis.prototype);
  }
}
