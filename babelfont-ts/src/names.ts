import { Names as INames } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";

export interface Names extends WithCamelCase<INames> {}
export class Names {
  constructor(data: INames) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Names.prototype) as Names;
  }
}
