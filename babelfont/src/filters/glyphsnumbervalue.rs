use std::collections::HashMap;

use crate::{
    convertors::glyphs3::{KEY_NUMBER_NAMES, KEY_NUMBER_VALUES},
    filters::FontFilter,
    Features,
};
use fea_rs_ast::{AsFea, FeatureFile, LayoutVisitor};
use indexmap::IndexMap;
use smol_str::SmolStr;

#[derive(Default)]
/// A filter that converts Glyphs number values to variable scalars in feature code
pub struct GlyphsNumberValue;

impl GlyphsNumberValue {
    /// Create a new GlyphsNumberValue filter
    pub fn new() -> Self {
        GlyphsNumberValue
    }
}

impl FontFilter for GlyphsNumberValue {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let features = font.features.to_fea();
        let glyph_names = font
            .glyphs
            .iter()
            .map(|g| g.name.as_str())
            .collect::<Vec<_>>();
        let mut feature_file: FeatureFile =
            FeatureFile::new_from_fea(&features, Some(&glyph_names), font.source.clone())
                .map_err(|e| crate::BabelfontError::FilterError(e.to_string()))?;

        let number_names = font.format_specific.get(KEY_NUMBER_NAMES).map(|v| {
            v.as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|item| item.as_str())
                .map(|s| s.to_string())
                .collect::<Vec<String>>()
        });
        if let Some(names) = number_names {
            let axes = font.fontdrasil_axes()?;
            let mut variables: HashMap<String, fea_rs_ast::Metric> = HashMap::new();
            for (index, name) in names.iter().enumerate() {
                let mut location_values: Vec<(IndexMap<SmolStr, i16>, i16)> = vec![];
                for master in font.masters.iter() {
                    let location_as_map = master
                        .location
                        .to_user(&axes)?
                        .iter()
                        .map(|(axis, value)| {
                            (SmolStr::new(axis.to_string()), value.to_f64() as i16)
                        })
                        .collect::<IndexMap<SmolStr, i16>>();
                    let number_values = master
                        .format_specific
                        .get(KEY_NUMBER_VALUES)
                        .and_then(|v| v.as_array());
                    let value_at_location = number_values
                        .and_then(|arr| arr.get(index))
                        .and_then(|v| v.as_f64())
                        .map(|i| i as i16)
                        .unwrap_or(0);
                    location_values.push((location_as_map, value_at_location));
                }
                let var_metric = fea_rs_ast::Metric::Variable(location_values);
                variables.insert(name.clone(), var_metric);
            }
            let mut visitor = GlyphsNumberValueVisitor(variables);
            visitor.visit(&mut feature_file).map_err(|e| {
                crate::BabelfontError::FilterError(format!(
                    "Error during feature replacement: {}",
                    e
                ))
            })?;
            font.features = Features::from_fea(&feature_file.as_fea(""));
        } else {
            log::debug!("No Glyphs number names found; skipping GlyphsNumberValue filter");
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(GlyphsNumberValue::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("glyphsnumbervalue")
            .long("convert-glyphs-number-values")
            .help("Convert Glyphs number values to variable scalars in feature code")
            .action(clap::ArgAction::SetTrue)
    }
}

struct GlyphsNumberValueVisitor(HashMap<String, fea_rs_ast::Metric>);

impl LayoutVisitor for GlyphsNumberValueVisitor {
    fn visit_statement(&mut self, statement: &mut fea_rs_ast::Statement) -> bool {
        match statement {
            fea_rs_ast::Statement::ValueRecordDefinition(vrd) => {
                self.visit_value_record_definition(vrd)
            }
            fea_rs_ast::Statement::PairPos(pp) => self.visit_pair_pos(pp),
            fea_rs_ast::Statement::SinglePos(sp) => self.visit_single_pos(sp),
            fea_rs_ast::Statement::CursivePos(cp) => self.visit_cursive_pos(cp),
            fea_rs_ast::Statement::MarkBasePos(mbp) => self.visit_mark_base_pos(mbp),
            fea_rs_ast::Statement::MarkLigPos(mlp) => self.visit_mark_lig_pos(mlp),
            fea_rs_ast::Statement::MarkMarkPos(mmp) => self.visit_mark_mark_pos(mmp),
            _ => true,
        }
    }
}

impl GlyphsNumberValueVisitor {
    fn convert_metric(&self, metric: &mut fea_rs_ast::Metric) {
        if let fea_rs_ast::Metric::GlyphsAppNumber(n) = metric {
            // remove the dollar sign if present
            let n = n.trim_start_matches('$');
            if let Some(var_metric) = self.0.get(n) {
                *metric = var_metric.clone();
            }
        }
    }

    fn convert_option_metric(&self, metric: &mut Option<fea_rs_ast::Metric>) {
        if let Some(m) = metric {
            self.convert_metric(m);
        }
    }

    fn convert_value_record(&mut self, vr: &mut fea_rs_ast::ValueRecord) {
        self.convert_option_metric(&mut vr.x_advance);
        self.convert_option_metric(&mut vr.y_advance);
        self.convert_option_metric(&mut vr.x_placement);
        self.convert_option_metric(&mut vr.y_placement);
    }

    fn convert_anchor(&mut self, anchor: &mut fea_rs_ast::Anchor) {
        self.convert_metric(&mut anchor.x);
        self.convert_metric(&mut anchor.y);
    }

    fn convert_option_anchor(&mut self, anchor: &mut Option<fea_rs_ast::Anchor>) {
        if let Some(a) = anchor {
            self.convert_anchor(a);
        }
    }

    fn visit_value_record_definition(
        &mut self,
        vrd: &mut fea_rs_ast::ValueRecordDefinition,
    ) -> bool {
        self.convert_value_record(&mut vrd.value);

        true
    }

    fn visit_pair_pos(&mut self, pp: &mut fea_rs_ast::PairPosStatement) -> bool {
        self.convert_value_record(&mut pp.value_record_1);
        if let Some(vr2) = &mut pp.value_record_2 {
            self.convert_value_record(vr2);
        }
        true
    }

    fn visit_single_pos(&mut self, sp: &mut fea_rs_ast::SinglePosStatement) -> bool {
        for (_gc, pos) in sp.pos.iter_mut() {
            if let Some(pos) = pos {
                self.convert_value_record(pos);
            }
        }
        true
    }

    fn visit_cursive_pos(&mut self, cp: &mut fea_rs_ast::CursivePosStatement) -> bool {
        self.convert_option_anchor(&mut cp.entry);
        self.convert_option_anchor(&mut cp.exit);
        true
    }

    fn visit_mark_base_pos(&mut self, mbp: &mut fea_rs_ast::MarkBasePosStatement) -> bool {
        for (anchor, _mark_class) in mbp.marks.iter_mut() {
            self.convert_anchor(anchor);
        }
        true
    }

    fn visit_mark_lig_pos(&mut self, mlp: &mut fea_rs_ast::MarkLigPosStatement) -> bool {
        for level1 in mlp.marks.iter_mut() {
            for (anchor, _mark_class) in level1.iter_mut() {
                self.convert_anchor(anchor);
            }
        }
        true
    }

    fn visit_mark_mark_pos(&mut self, mmp: &mut fea_rs_ast::MarkMarkPosStatement) -> bool {
        for (anchor, _mark_class) in mmp.marks.iter_mut() {
            self.convert_anchor(anchor);
        }
        true
    }
}
