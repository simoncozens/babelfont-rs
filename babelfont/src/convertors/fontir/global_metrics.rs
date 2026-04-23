use crate::{Font, MetricType};
use fontdrasil::orchestration::{Access, Work};
use fontir::{
    error::{BadSourceKind, Error},
    ir::{GlobalMetric, GlobalMetricsBuilder},
    orchestration::{Context, WorkId},
};
use std::sync::Arc;

// fn get_number_values(
//     fontinfo: &FontInfo,
//     font: &Font,
// ) -> Option<HashMap<NormalizedLocation, BTreeMap<SmolStr, OrderedFloat<f64>>>> {
//     if font.default_master().number_values.is_empty() {
//         return None;
//     }
//     let values = font
//         .masters
//         .iter()
//         .map(|m| {
//             let location = fontinfo.locations.get(&m.axes_values).cloned().unwrap();
//             (location, m.number_values.clone())
//         })
//         .collect();
//     Some(values)
// }

#[derive(Debug)]
pub(crate) struct GlobalMetricWork(pub Arc<Font>);

impl Work<Context, WorkId, Error> for GlobalMetricWork {
    fn id(&self) -> WorkId {
        WorkId::GlobalMetrics
    }

    fn read_access(&self) -> Access<WorkId> {
        Access::Variant(WorkId::StaticMetadata)
    }

    fn exec(&self, context: &Context) -> Result<(), Error> {
        log::debug!(
            "Global metrics for {}",
            self.0
                .names
                .family_name
                .get_default()
                .unwrap_or(&"<nameless family>".to_string())
        );

        let static_metadata = context.static_metadata.get();
        let mut metrics = GlobalMetricsBuilder::new();
        let axes = self.0.fontdrasil_axes().map_err(|e| {
            Error::BadSource(fontir::error::BadSource::new(
                self.0.source.clone().unwrap_or("unknown source".into()),
                BadSourceKind::Custom(format!("Error converting axes for global metrics: {e}")),
            ))
        })?;

        for master in self.0.masters.iter() {
            let pos = master.location.to_normalized(&axes)?;
            // glyphsLib <https://github.com/googlefonts/glyphsLib/blob/1cb4fc5ae2/Lib/glyphsLib/classes.py#L1590-L1601>
            let cap_height = master
                .metrics
                .get(&MetricType::CapHeight)
                .copied()
                .unwrap_or(700);
            let x_height = master
                .metrics
                .get(&MetricType::XHeight)
                .copied()
                .unwrap_or(500);
            let ascender = master
                .metrics
                .get(&MetricType::Ascender)
                .copied()
                .unwrap_or(800);
            let descender = master
                .metrics
                .get(&MetricType::Descender)
                .copied()
                .unwrap_or(-200);

            metrics.set_if_some(GlobalMetric::CapHeight, pos.clone(), Some(cap_height));
            metrics.set_if_some(GlobalMetric::XHeight, pos.clone(), Some(x_height));

            // Some .glyphs files have a negative win descent so abs()
            // we don't use the macro here because of the special abs() logic
            metrics.set_if_some(
                GlobalMetric::Os2WinDescent,
                pos.clone(),
                master
                    .metrics
                    .get(&MetricType::WinDescent)
                    .map(|v| v.abs() as f64),
            );

            macro_rules! set_metric {
                // the most common pattern
                ($variant:ident, $field_name:ident) => {
                    set_metric!(
                        $variant,
                        master
                            .metrics
                            .get(&MetricType::$field_name)
                            .map(|v| *v as f64)
                    )
                };
                // a few fields have a manual default though
                ($variant:ident, $field_name:ident, $fallback:literal) => {
                    set_metric!(
                        $variant,
                        master
                            .metrics
                            .get(&MetricType::$field_name)
                            .map(|v| *v as f64)
                            .or(Some($fallback.into()))
                    )
                };
                // base case, both branches above resolve to this
                ($variant:ident, $getter:expr ) => {
                    metrics.set_if_some(GlobalMetric::$variant, pos.clone(), $getter)
                };
            }
            set_metric!(Os2TypoAscender, TypoAscender);
            set_metric!(Os2TypoDescender, TypoDescender);
            set_metric!(Os2TypoLineGap, TypoLineGap);
            set_metric!(Os2WinAscent, WinAscent);
            set_metric!(StrikeoutPosition, StrikeoutPosition);
            set_metric!(StrikeoutSize, StrikeoutSize);
            set_metric!(SubscriptXOffset, SubscriptXOffset);
            set_metric!(SubscriptXSize, SubscriptXSize);
            set_metric!(SubscriptYOffset, SubscriptYOffset);
            set_metric!(SubscriptYSize, SubscriptYSize);
            set_metric!(SuperscriptXOffset, SuperscriptXOffset);
            set_metric!(SuperscriptXSize, SuperscriptXSize);
            set_metric!(SuperscriptYOffset, SuperscriptYOffset);
            set_metric!(SuperscriptYSize, SuperscriptYSize);
            set_metric!(HheaAscender, HheaAscender);
            set_metric!(HheaDescender, HheaDescender);
            set_metric!(HheaLineGap, HheaLineGap);
            set_metric!(CaretSlopeRun, HheaCaretSlopeRun);
            set_metric!(CaretSlopeRise, HheaCaretSlopeRise);
            set_metric!(CaretOffset, HheaCaretOffset);
            // 50.0 is the Glyphs default <https://github.com/googlefonts/glyphsLib/blob/9d5828d874110c42dfc5f542db8eb84f88641eb5/Lib/glyphsLib/builder/custom_params.py#L1136-L1156>
            set_metric!(UnderlineThickness, UnderlineThickness, 50.0);
            // -100.0 is the Glyphs default <https://github.com/googlefonts/glyphsLib/blob/9d5828d874110c42dfc5f542db8eb84f88641eb5/Lib/glyphsLib/builder/custom_params.py#L1136-L1156>
            set_metric!(UnderlinePosition, UnderlinePosition, -100.0);
            // set_metric!(VheaCaretSlopeRise, VheaCaretSlopeRise);
            // set_metric!(VheaCaretSlopeRun, VheaCaretSlopeRun);
            // set_metric!(VheaCaretOffset, VheaCaretOffset);

            // // https://github.com/googlefonts/glyphsLib/blob/c4db6b981d577f456d64ebe9993818770e170454/Lib/glyphsLib/builder/masters.py#L74-L92
            // metrics.set(
            //     GlobalMetric::VheaAscender,
            //     pos.clone(),
            //     master
            //         .custom_parameters
            //         .vhea_ascender
            //         .or(font.custom_parameters.vhea_ascender)
            //         .map(|v| v as f64)
            //         .unwrap_or(font.upm as f64 / 2.0),
            // );
            // metrics.set(
            //     GlobalMetric::VheaDescender,
            //     pos.clone(),
            //     master
            //         .custom_parameters
            //         .vhea_descender
            //         .or(font.custom_parameters.vhea_descender)
            //         .map(|v| v as f64)
            //         .unwrap_or(-(font.upm as f64 / 2.0)),
            // );
            metrics.set(
                GlobalMetric::VheaLineGap,
                pos.clone(),
                master
                    .metrics
                    .get(&MetricType::Custom("vheaLineGap".into()))
                    .map(|v| *v as f64)
                    .unwrap_or(self.0.upm as f64),
            );

            metrics.populate_defaults(
                &pos,
                static_metadata.units_per_em,
                Some(x_height.into()),
                Some(ascender.into()),
                Some(descender.into()),
                // turn clockwise angle counter-clockwise
                master
                    .metrics
                    .get(&MetricType::ItalicAngle)
                    .map(|v| -*v as f64),
            );
        }

        let global_metrics = metrics.build(&static_metadata.axes)?;
        // Ensure that one of the masters has that location
        assert!(
            self.0
                .masters
                .iter()
                .any(|m| m.location.to_normalized(&axes).ok().as_ref()
                    == Some(static_metadata.default_location())),
            "No master at default location {:?}",
            static_metadata.default_location()
        );
        global_metrics.at(static_metadata.default_location());

        context.global_metrics.set(global_metrics);
        // Check we have a default location; this'll panic if we don't
        Ok(())
    }
}
