import { Master as IMaster } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import { getClassConstructor } from "./registry";
import { Guide } from "./guide";
import { OTValue } from "./otvalue";

export interface Master extends WithCamelCase<IMaster> {
  customOtValues?: OTValue[];
}
export class Master {
  constructor(data: IMaster) {
    if (data.guides) {
      const GuideClass = getClassConstructor("Guide", Guide);
      data.guides = data.guides.map((g) => new GuideClass(g));
    }
    if (data.custom_ot_values) {
      const OTValueClass = getClassConstructor("OTValue", OTValue);
      data.custom_ot_values = data.custom_ot_values.map(
        (otv) => new OTValueClass(otv)
      );
    }
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Master.prototype);
  }
}
