import { Guide as IGuide } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import type { Layer } from "./layer";
import type { Master } from "./master";
import { WithParent, ensureParentAccessors } from "./parent";

export interface Guide
  extends WithCamelCase<IGuide>, WithParent<Layer | Master> {}
export class Guide {
  constructor(data: IGuide) {
    Object.assign(this, data);
    ensureParentAccessors(this);
    return createCaseConvertingProxy(this, Guide.prototype);
  }
}
