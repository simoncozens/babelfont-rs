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

impl DecomposedAffine {
    /// The same transform, decomposed in the given order
    pub fn in_order(&self, order: TransformOrder) -> DecomposedAffine {
        if self.order == order {
            *self
        } else {
            Self::from_affine_in_order(self.as_affine(), order)
        }
    }

    /// An affine decomposed in the given order. In Glyphs order (a skew after
    /// the rotation) some affines need a skew in both x and y, or have none;
    /// those keep the other order, which every affine has.
    pub fn from_affine_in_order(affine: kurbo::Affine, order: TransformOrder) -> DecomposedAffine {
        match order {
            TransformOrder::RestOfTheWorld => affine.into(),
            TransformOrder::Glyphs => glyphs_order(affine).unwrap_or_else(|| {
                log::warn!(
                    "{:?} has no decomposition as Glyphs states transforms",
                    affine.as_coeffs()
                );
                affine.into()
            }),
        }
    }

    /// The same transform turned half a turn more with both scales negated
    /// (R(θ + π) = -R(θ)), the rotation within half a turn of `near`
    fn nearest_rotation(&self, near: f64) -> DecomposedAffine {
        use std::f64::consts::{PI, TAU};
        let unwrap = |t: DecomposedAffine| DecomposedAffine {
            rotation: t.rotation - ((t.rotation - near) / TAU).round() * TAU,
            ..t
        };
        let same = unwrap(*self);
        let turned = unwrap(DecomposedAffine {
            rotation: self.rotation + PI,
            scale: (-self.scale.0, -self.scale.1),
            ..*self
        });
        if (turned.rotation - near).abs() < (same.rotation - near).abs() {
            turned
        } else {
            same
        }
    }

    /// Masters' transforms as interpolation needs them: in one order, and with
    /// rotations that do not jump half a turn between masters (an affine turned
    /// 100° decomposes as one turned -80° with both scales negated)
    pub fn compatible(transforms: &[DecomposedAffine]) -> Vec<DecomposedAffine> {
        let Some(first) = transforms.first() else {
            return vec![];
        };
        transforms
            .iter()
            .map(|t| t.in_order(first.order).nearest_rotation(first.rotation))
            .collect()
    }
}

/// The roots of a k² + b k + c = 0, the smallest first
fn quadratic_roots(a: f64, b: f64, c: f64) -> Vec<f64> {
    let mut roots = if a.abs() < 1e-12 {
        if b.abs() < 1e-12 {
            vec![]
        } else {
            vec![-c / b]
        }
    } else {
        let discriminant = b * b - 4.0 * a * c;
        if discriminant < 0.0 {
            vec![]
        } else {
            let root = discriminant.sqrt();
            vec![(-b + root) / (2.0 * a), (-b - root) / (2.0 * a)]
        }
    };
    roots.sort_by(|x, y| x.abs().total_cmp(&y.abs()));
    roots
}

/// An affine as Glyphs states a component's transform: translate · skew ·
/// rotate · scale. With a skew in x only, K⁻¹ · M must be a rotation times a
/// scale, whose columns are perpendicular: that is a quadratic in the skew.
/// Likewise in y; failing both, no rotation and a skew in both.
fn glyphs_order(affine: kurbo::Affine) -> Option<DecomposedAffine> {
    let [a, b, c, d, t_x, t_y] = affine.as_coeffs();
    // a rotation times a scale, as R(θ) · S(sx, sy) with sx's sign the mirror's
    let rotate_scale = |n: [f64; 4]| -> Option<(f64, (f64, f64))> {
        let [na, nb, nc, nd] = n;
        let sx = if na < 0.0 {
            -na.hypot(nb)
        } else {
            na.hypot(nb)
        };
        if sx == 0.0 {
            return None;
        }
        Some(((nb / sx).atan2(na / sx), (sx, (na * nd - nb * nc) / sx)))
    };
    let mut candidates = vec![];
    for k in quadratic_roots(b * d, -(a * d + b * c), a * c + b * d) {
        if let Some((rotation, scale)) = rotate_scale([a - k * b, b, c - k * d, d]) {
            candidates.push((rotation, scale, (k.atan(), 0.0)));
        }
    }
    for k in quadratic_roots(a * c, -(a * d + b * c), b * d + a * c) {
        if let Some((rotation, scale)) = rotate_scale([a, b - k * a, c, d - k * c]) {
            candidates.push((rotation, scale, (0.0, k.atan())));
        }
    }
    if a != 0.0 && d != 0.0 {
        candidates.push((0.0, (a, d), ((c / d).atan(), (b / a).atan())));
    }
    candidates
        .into_iter()
        .map(|(rotation, scale, skew)| DecomposedAffine {
            translation: (t_x, t_y),
            scale,
            rotation,
            skew,
            order: TransformOrder::Glyphs,
        })
        .find(|t| {
            t.as_affine()
                .as_coeffs()
                .iter()
                .zip(affine.as_coeffs())
                .all(|(x, y)| (x - y).abs() <= 1e-9 * (1.0 + y.abs()))
        })
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
            // the first column is zero: R(θ) · S(0, sy) leaves the second column
            // sy · (-sin θ, cos θ), so θ comes from it
            let sy = c.hypot(d);
            return DecomposedAffine {
                translation: (t_x, t_y),
                scale: (0.0, sy),
                rotation: if sy == 0.0 { 0.0 } else { (-c).atan2(d) },
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
            DecomposedAffine::from_affine_in_order(
                kurbo::Affine::new([t.m11, t.m12, t.m21, t.m22, t.t_x, t.t_y].map(|v| v as f64)),
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

    #[test]
    fn test_affines_survive_decomposition_in_glyphs_order() {
        for coeffs in [
            [1.0, 0.0, 0.0, 1.0, 5.0, 7.0],
            [0.0, 1.0, -1.0, 0.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            [-0.5, 0.4, 0.2, 1.1, 0.0, 0.0],
            [0.8, 0.0, 0.3, 0.8, 10.0, 20.0],
            [0.8, 0.3, 0.0, 0.8, 0.0, 0.0],
            [1.2, 0.2, -0.4, 0.9, 3.0, 4.0],
            [0.8, 0.2736, -0.1411, 0.7518, 0.0, 0.0],
        ] {
            let decomposed = DecomposedAffine::from_affine_in_order(
                kurbo::Affine::new(coeffs),
                TransformOrder::Glyphs,
            );
            assert_eq!(decomposed.order, TransformOrder::Glyphs, "{coeffs:?}");
            let back = decomposed.as_affine().as_coeffs();
            assert!(
                back.iter().zip(coeffs).all(|(a, b)| (a - b).abs() < 1e-9),
                "{coeffs:?} came back as {back:?}"
            );
        }
        // Glyphs' own: angle 20, scale 0.8, slant 10
        let decomposed = DecomposedAffine::from_affine_in_order(
            kurbo::Affine::skew(10f64.to_radians().tan(), 0.0)
                * kurbo::Affine::rotate(20f64.to_radians())
                * kurbo::Affine::scale(0.8),
            TransformOrder::Glyphs,
        );
        assert!((decomposed.rotation.to_degrees() - 20.0).abs() < 1e-9);
        assert!((decomposed.skew.0.to_degrees() - 10.0).abs() < 1e-9);
        assert!((decomposed.scale.0 - 0.8).abs() < 1e-9 && (decomposed.scale.1 - 0.8).abs() < 1e-9);
    }
}
