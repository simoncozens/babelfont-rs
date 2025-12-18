import { Guide as IGuide } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";

export interface Guide extends WithCamelCase<IGuide> {}
export class Guide {
  constructor(data: IGuide) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Guide.prototype);
  }
}
