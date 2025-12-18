import { Instance as IInstance } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import { getClassConstructor } from "./registry";
import { Names } from "./names";
import type { Font } from "./font";
import { WithParent, ensureParentAccessors } from "./parent";

export interface Instance extends WithCamelCase<IInstance>, WithParent<Font> {}
export class Instance {
  constructor(data: IInstance) {
    ensureParentAccessors(this);
    if (data.custom_names) {
      const NamesClass = getClassConstructor("Names", Names);
      data.custom_names = new NamesClass(data.custom_names);
    }
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Instance.prototype);
  }
}
