import { OTValue as IOTValue } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import type { Master } from "./master";
import { WithParent, ensureParentAccessors } from "./parent";

export interface OTValue extends WithCamelCase<IOTValue>, WithParent<Master> {}
export class OTValue {
  constructor(data: IOTValue) {
    Object.assign(this, data);
    ensureParentAccessors(this);
    return createCaseConvertingProxy(this, OTValue.prototype);
  }
}
