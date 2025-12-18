import { OTValue as IOTValue } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";

export interface OTValue extends WithCamelCase<IOTValue> {}
export class OTValue {
  constructor(data: IOTValue) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, OTValue.prototype);
  }
}
