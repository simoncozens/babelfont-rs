import { Master as IMaster } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import { getClassConstructor } from "./registry";
import { Guide } from "./guide";
import { OTValue } from "./otvalue";
import type { Font } from "./font";
import { WithParent, ensureParentAccessors, setParent } from "./parent";

export interface Master extends WithCamelCase<IMaster>, WithParent<Font> {
  customOtValues?: OTValue[];
}
export class Master {
  constructor(data: IMaster) {
    ensureParentAccessors(this);
    if (data.guides) {
      const GuideClass = getClassConstructor("Guide", Guide);
      data.guides = data.guides.map((g) => {
        const guide = new GuideClass(g);
        setParent(guide as unknown as WithParent<Master>, this);
        return guide;
      });
    }
    if (data.custom_ot_values) {
      const OTValueClass = getClassConstructor("OTValue", OTValue);
      data.custom_ot_values = data.custom_ot_values.map((otv) => {
        const otValue = new OTValueClass(otv);
        setParent(otValue as unknown as WithParent<Master>, this);
        return otValue;
      });
    }
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Master.prototype);
  }
}
