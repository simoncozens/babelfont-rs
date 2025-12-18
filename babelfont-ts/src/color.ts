import { Color as IColor } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";

export interface Color extends WithCamelCase<IColor> {}
export class Color {
  constructor(data: IColor) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Color.prototype);
  }
}
