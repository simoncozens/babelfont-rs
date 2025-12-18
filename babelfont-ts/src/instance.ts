import { Instance as IInstance } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import { getClassConstructor } from "./registry";
import { Names } from "./names";

export interface Instance extends WithCamelCase<IInstance> {}
export class Instance {
  constructor(data: IInstance) {
    if (data.custom_names) {
      const NamesClass = getClassConstructor("Names", Names);
      data.custom_names = new NamesClass(data.custom_names);
    }
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Instance.prototype);
  }
}
