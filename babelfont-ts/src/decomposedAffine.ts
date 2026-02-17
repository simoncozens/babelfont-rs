import { DecomposedAffine as IDecomposedAffine } from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import {
  compose,
  Matrix,
  rotate,
  scale,
  skew,
  translate,
} from "transformation-matrix";

export interface DecomposedAffine extends WithCamelCase<IDecomposedAffine> {
  toAffine(): [number, number, number, number, number, number];
  toAffineMatrix(): Matrix;
}
export class DecomposedAffine {
  constructor(data: IDecomposedAffine) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, DecomposedAffine.prototype);
  }

  toAffineMatrix(): Matrix {
    const angle = this.rotation;
    const [skewX, skewY] = this.skew || [0, 0];
    const [scaleX, scaleY] = this.scale || [1, 1];
    if (this.order == "Glyphs") {
      // Glyphs order: translate, rotate, skew, scale
      return compose(
        translate(
          this.translation ? this.translation[0] : 0,
          this.translation ? this.translation[1] : 0,
        ),
        rotate(angle || 0),
        skew(skewX, skewY),
        scale(scaleX, scaleY),
      );
    } else {
      // Default order: translate, scale, skew, rotate
      return compose(
        translate(
          this.translation ? this.translation[0] : 0,
          this.translation ? this.translation[1] : 0,
        ),
        scale(scaleX, scaleY),
        skew(skewX, skewY),
        rotate(angle || 0),
      );
    }
  }

  toAffine(): [number, number, number, number, number, number] {
    const matrix = this.toAffineMatrix();
    return [matrix.a, matrix.b, matrix.c, matrix.d, matrix.e, matrix.f];
  }
}
