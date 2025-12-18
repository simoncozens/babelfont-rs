import { Position as IPosition } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";

export interface Position extends WithCamelCase<IPosition> {}
export class Position {
  constructor(data: IPosition) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Position.prototype);
  }
}
