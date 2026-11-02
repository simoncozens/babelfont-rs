use serde::{Deserialize, Serialize};
use typeshare::typeshare;

/// The order in which transform operations should be applied
// See https://github.com/googlefonts/fontc/issues/1127
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[typeshare]
pub enum TransformOrder {
    /// Glyphs order: translate → skew → rotate → scale
    Glyphs,
    #[default]
    /// Translate → rotate → scale → skew
    RestOfTheWorld,
}

/// A decomposed affine transformation with separate translation, rotation, scale, and skew components
#[derive(Debug, Clone, Serialize, Deserialize, Copy, PartialEq)]
#[typeshare]
pub struct DecomposedAffine {
    #[serde(default, skip_serializing_if = "crate::serde_helpers::is_default")]
    #[typeshare(typescript(type = "[number, number]"))]
    #[typeshare(python(type = "(float, float)"))]
    /// The translation component of the transformation (tx, ty)
    pub translation: (f64, f64),
    #[serde(
        default = "crate::serde_helpers::one_one",
        skip_serializing_if = "crate::serde_helpers::is_one_one"
    )]
    #[typeshare(typescript(type = "[number, number]"))]
    #[typeshare(python(type = "(float, float)"))]
    /// The scale component of the transformation (sx, sy)
    pub scale: (f64, f64),
    #[serde(default, skip_serializing_if = "crate::serde_helpers::is_default")]
    /// The rotation component of the transformation in radians
    pub rotation: f64,
    #[serde(default, skip_serializing_if = "crate::serde_helpers::is_default")]
    #[typeshare(typescript(type = "[number, number]"))]
    #[typeshare(python(type = "(float, float)"))]
    /// The skew component of the transformation (skew_x, skew_y) in radians
    pub skew: (f64, f64),
    #[serde(default, skip_serializing_if = "crate::serde_helpers::is_default")]
    /// The order in which the transformation components should be applied
    pub order: TransformOrder,
}

impl Default for DecomposedAffine {
    fn default() -> Self {
        Self {
            translation: (0.0, 0.0),
            scale: (1.0, 1.0),
            rotation: 0.0,
            skew: (0.0, 0.0),
            order: TransformOrder::default(),
        }
    }
}

impl DecomposedAffine {
    /// Create a new identity DecomposedAffine with the given transform order
    pub fn new(order: TransformOrder) -> Self {
        Self {
            order,
            ..Default::default()
        }
    }

    /// Convert to a kurbo::Affine using the appropriate operation order
    pub fn as_affine(&self) -> kurbo::Affine {
        let translate = kurbo::Vec2::new(self.translation.0, self.translation.1);

        match self.order {
            // skew is an angle; kurbo's skew takes the shear, its tangent
            TransformOrder::Glyphs => {
                kurbo::Affine::translate(translate)
                    * kurbo::Affine::skew(self.skew.0.tan(), self.skew.1.tan())
                    * kurbo::Affine::rotate(self.rotation)
                    * kurbo::Affine::scale_non_uniform(self.scale.0, self.scale.1)
            }
            TransformOrder::RestOfTheWorld => {
                kurbo::Affine::translate(translate)
                    * kurbo::Affine::rotate(self.rotation)
                    * kurbo::Affine::scale_non_uniform(self.scale.0, self.scale.1)
                    * kurbo::Affine::skew(self.skew.0.tan(), self.skew.1.tan())
            }
        }
    }
}

fn decompose<T: Into<f64>>(
    scale_x: T,
    scale_xy: T,
    scale_yx: T,
    scale_y: T,
    t_x: T,
    t_y: T,
    order: TransformOrder,
) -> DecomposedAffine {
    let mut a: f64 = scale_x.into();
    let mut b: f64 = scale_xy.into();
    let c: f64 = scale_yx.into();
    let d: f64 = scale_y.into();
    let t_x: f64 = t_x.into();
    let t_y: f64 = t_y.into();

    // Remove a possible sign on a to mirror the reference decomposition.
    let sx_sign = if a == 0.0 { 1.0_f64 } else { a.signum() };
    if sx_sign < 0.0 {
        a *= sx_sign;
        b *= sx_sign;
    }

    let delta: f64 = a * d - b * c;
    let translation = (t_x, t_y);
    let (rotation, scale, skew) = if a != 0.0 || b != 0.0 {
        let r = (a * a + b * b).sqrt();
        // Use atan2 to keep rotation sign consistent with the matrix [[a, b], [c, d]]
        let angle = if delta >= 0.0 {
            (-b).atan2(a)
        } else {
            b.atan2(a)
        };
        let scale_x_out = r * sx_sign;
        let scale_y_out = delta / r;
        // Skew on X axis; skew_y is always zero in this decomposition.
        let skew_x = ((a * c + b * d) / (r * r)).atan() * sx_sign;
        (angle, (scale_x_out, scale_y_out), (skew_x, 0.0))
    } else if c != 0.0 || d != 0.0 {
        let s = (c * c + d * d).sqrt();
        let angle = if delta >= 0.0 {
            c.atan2(d)
        } else {
            (-c).atan2(d)
        };
        let scale_x_out = delta / s;
        let scale_y_out = s;
        (angle, (scale_x_out, scale_y_out), (0.0, 0.0))
    } else {
        (0.0, (0.0, 0.0), (0.0, 0.0))
    };
    DecomposedAffine {
        translation,
        scale,
        rotation,
        skew,
        order,
    }
}

impl From<kurbo::Affine> for DecomposedAffine {
    /// The affine as rotation · scale · skew in x (the order `as_affine` puts
    /// them back in): x' = a x + c y, y' = b x + d y is
    /// R(θ) · S(sx, sy) · K(k), with sx and θ from the first column, and k and
    /// sy what the second column leaves.
    fn from(t: kurbo::Affine) -> Self {
        let [a, b, c, d, t_x, t_y] = t.as_coeffs();
        // a mirrored first column is a negative scale, not a half turn, so
        // flips come back exact
        let sx = if a < 0.0 { -a.hypot(b) } else { a.hypot(b) };
        if sx == 0.0 {
            // the first column is zero: no rotation to find, a degenerate scale
            return DecomposedAffine {
                translation: (t_x, t_y),
                scale: (0.0, c.hypot(d)),
                rotation: 0.0,
                skew: (0.0, 0.0),
                order: TransformOrder::RestOfTheWorld,
            };
        }
        let rotation = (b / sx).atan2(a / sx);
        let (sin, cos) = rotation.sin_cos();
        let shear = (cos * c + sin * d) / sx;
        let sy = cos * d - sin * c;
        DecomposedAffine {
            translation: (t_x, t_y),
            scale: (sx, sy),
            rotation,
            skew: (shear.atan(), 0.0),
            order: TransformOrder::RestOfTheWorld,
        }
    }
}

impl From<DecomposedAffine> for kurbo::Affine {
    fn from(t: DecomposedAffine) -> Self {
        t.as_affine()
    }
}

#[cfg(feature = "glyphs")]
mod glyphs {
    use super::*;
    use glyphslib::glyphs2;
    impl From<&glyphs2::Transform> for DecomposedAffine {
        fn from(t: &glyphs2::Transform) -> Self {
            decompose(
                t.m11,
                t.m12,
                t.m21,
                t.m22,
                t.t_x,
                t.t_y,
                TransformOrder::Glyphs,
            )
        }
    }
}

#[cfg(feature = "fontra")]
mod fontra {
    use super::*;
    use crate::convertors::fontra::DecomposedTransform as FontraTransform;
    impl From<DecomposedAffine> for FontraTransform {
        fn from(t: DecomposedAffine) -> Self {
            FontraTransform {
                translate_x: t.translation.0 as f32,
                translate_y: t.translation.1 as f32,
                rotation: t.rotation as f32,
                scale_x: t.scale.0 as f32,
                scale_y: t.scale.1 as f32,
                skew_x: t.skew.0 as f32,
                skew_y: t.skew.1 as f32,
                t_center_x: 0.0,
                t_center_y: 0.0,
            }
        }
    }
    impl From<FontraTransform> for DecomposedAffine {
        fn from(t: FontraTransform) -> Self {
            DecomposedAffine {
                translation: (t.translate_x as f64, t.translate_y as f64),
                scale: (t.scale_x as f64, t.scale_y as f64),
                rotation: t.rotation as f64,
                skew: (t.skew_x as f64, t.skew_y as f64),
                order: TransformOrder::RestOfTheWorld,
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_affines_survive_decomposition() {
        // kurbo's order: [xx, yx, xy, yy, dx, dy], x' = xx x + xy y, y' = yx x + yy y
        for coeffs in [
            [1.0, 0.0, 0.0, 1.0, 5.0, 7.0],
            [0.0, 1.0, -1.0, 0.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0, -1.0, 2.0, 0.0],
            [-0.5, 0.4, 0.2, 1.1, 0.0, 0.0],
            [0.8, 0.0, 0.3, 0.8, 10.0, 20.0],
            [0.8, 0.3, 0.0, 0.8, 0.0, 0.0],
            [1.2, 0.2, -0.4, 0.9, 3.0, 4.0],
        ] {
            let back = DecomposedAffine::from(kurbo::Affine::new(coeffs))
                .as_affine()
                .as_coeffs();
            assert!(
                back.iter().zip(coeffs).all(|(a, b)| (a - b).abs() < 1e-9),
                "{coeffs:?} came back as {back:?}"
            );
        }
    }
}
