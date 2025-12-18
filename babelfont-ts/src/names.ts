import { Names as INames } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import type { Font } from "./font";
import { WithParent, ensureParentAccessors } from "./parent";

export interface Names extends WithCamelCase<INames>, WithParent<Font> {}
export class Names {
  constructor(data: INames) {
    Object.assign(this, data);
    ensureParentAccessors(this);
    return createCaseConvertingProxy(this, Names.prototype) as Names;
  }
}
