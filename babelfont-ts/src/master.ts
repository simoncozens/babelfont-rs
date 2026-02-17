import { Master as IMaster } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import { getClassConstructor } from "./registry";
import { Guide } from "./guide";
import type { Font } from "./font";
import { WithParent, ensureParentAccessors, setParent } from "./parent";
import { CustomOTValues } from "./otvalue";

export interface Master extends WithCamelCase<IMaster>, WithParent<Font> {}
export class Master {
  constructor(data: IMaster) {
    ensureParentAccessors(this);
    if (data.guides) {
      const GuideClass = getClassConstructor("Guide", Guide);
      data.guides = data.guides.map((g) => {
        const guide = new GuideClass(g);
        setParent((guide as unknown) as WithParent<Master>, this);
        return guide;
      });
    }
    if (data.custom_ot_values) {
      const CustomOTValuesClass = getClassConstructor(
        "CustomOTValues",
        CustomOTValues
      );
      data.custom_ot_values = new CustomOTValuesClass(data.custom_ot_values);
      setParent((data.custom_ot_values as unknown) as WithParent<Master>, this);
    }
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Master.prototype);
  }
}
