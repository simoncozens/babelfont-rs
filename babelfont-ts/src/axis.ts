import { Axis as IAxis } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import type { Font } from "./font";
import { WithParent, ensureParentAccessors } from "./parent";

export interface Axis extends WithCamelCase<IAxis>, WithParent<Font> {}
export class Axis {
  constructor(data: IAxis) {
    Object.assign(this, data);
    ensureParentAccessors(this);
    return createCaseConvertingProxy(this, Axis.prototype);
  }
}
