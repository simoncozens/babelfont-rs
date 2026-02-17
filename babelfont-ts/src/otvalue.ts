import { CustomOTValues as ICustomOTValues } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import type { Master } from "./master";
import { WithParent, ensureParentAccessors } from "./parent";

export interface CustomOTValues
  extends WithCamelCase<ICustomOTValues>, WithParent<Master> {}
export class CustomOTValues {
  constructor(data: ICustomOTValues) {
    Object.assign(this, data);
    ensureParentAccessors(this);
    return createCaseConvertingProxy(this, CustomOTValues.prototype);
  }
}
