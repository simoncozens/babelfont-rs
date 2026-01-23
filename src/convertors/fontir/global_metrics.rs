use crate::{Font, MetricType};
use fontdrasil::orchestration::{Access, Work};
use fontir::{
    error::Error,
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
        let axes = self.0.fontdrasil_axes()?;

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

            // // Some .glyphs files have a negative win descent so abs()
            // // we don't use the macro here because of the special abs() logic
            // metrics.set_if_some(
            //     GlobalMetric::Os2WinDescent,
            //     pos.clone(),
            //     master
            //         .custom_parameters
            //         .win_descent
            //         .or(font.custom_parameters.win_descent)
            //         .map(|v| v.abs() as f64),
            // );

            // macro_rules! set_metric {
            //     // the most common pattern
            //     ($variant:ident, $field_name:ident) => {
            //         set_metric!(
            //             $variant,
            //             master
            //                 .custom_parameters
            //                 .$field_name
            //                 .or(font.custom_parameters.$field_name)
            //                 .map(|v| v as f64)
            //         )
            //     };
            //     // a few fields have a manual default though
            //     ($variant:ident, $field_name:ident, $fallback:literal) => {
            //         set_metric!(
            //             $variant,
            //             master
            //                 .custom_parameters
            //                 .$field_name
            //                 .or(font.custom_parameters.$field_name)
            //                 .or(Some($fallback.into()))
            //         )
            //     };
            //     // base case, both branches above resolve to this
            //     ($variant:ident, $getter:expr ) => {
            //         metrics.set_if_some(GlobalMetric::$variant, pos.clone(), $getter)
            //     };
            // }
            // set_metric!(Os2TypoAscender, typo_ascender);
            // set_metric!(Os2TypoDescender, typo_descender);
            // set_metric!(Os2TypoLineGap, typo_line_gap);
            // set_metric!(Os2WinAscent, win_ascent);
            // set_metric!(StrikeoutPosition, strikeout_position);
            // set_metric!(StrikeoutSize, strikeout_size);
            // set_metric!(SubscriptXOffset, subscript_x_offset);
            // set_metric!(SubscriptXSize, subscript_x_size);
            // set_metric!(SubscriptYOffset, subscript_y_offset);
            // set_metric!(SubscriptYSize, subscript_y_size);
            // set_metric!(SuperscriptXOffset, superscript_x_offset);
            // set_metric!(SuperscriptXSize, superscript_x_size);
            // set_metric!(SuperscriptYOffset, superscript_y_offset);
            // set_metric!(SuperscriptYSize, superscript_y_size);
            // set_metric!(HheaAscender, hhea_ascender);
            // set_metric!(HheaDescender, hhea_descender);
            // set_metric!(HheaLineGap, hhea_line_gap);
            // set_metric!(CaretSlopeRun, hhea_caret_slope_run);
            // set_metric!(CaretSlopeRise, hhea_caret_slope_rise);
            // set_metric!(CaretOffset, hhea_caret_offset);
            // // 50.0 is the Glyphs default <https://github.com/googlefonts/glyphsLib/blob/9d5828d874110c42dfc5f542db8eb84f88641eb5/Lib/glyphsLib/builder/custom_params.py#L1136-L1156>
            // set_metric!(UnderlineThickness, underline_thickness, 50.0);
            // // -100.0 is the Glyphs default <https://github.com/googlefonts/glyphsLib/blob/9d5828d874110c42dfc5f542db8eb84f88641eb5/Lib/glyphsLib/builder/custom_params.py#L1136-L1156>
            // set_metric!(UnderlinePosition, underline_position, -100.0);
            // set_metric!(VheaCaretSlopeRise, vhea_caret_slope_rise);
            // set_metric!(VheaCaretSlopeRun, vhea_caret_slope_run);
            // set_metric!(VheaCaretOffset, vhea_caret_offset);

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

        context
            .global_metrics
            .set(metrics.build(&static_metadata.axes)?);
        Ok(())
    }
}
