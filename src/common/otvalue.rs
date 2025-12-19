use fontdrasil::types::Tag;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

/// Custom OpenType values that can be set per-master or per-font
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[typeshare]
pub struct CustomOTValues {
    // head table
    /// Head table flags field
    ///
    /// A bit field. The flags are:

    /// 0: Baseline for font at y=0
    /// 1: Left sidebearing at x=0
    /// 2: Instructions may depend on point size
    /// 3: Force ppem to integer values for all internal scaler math
    /// 4: Instructions may alter advance width (the advance width is not always the
    ///     same as the width of the glyph outline)
    /// 5-10: Not used
    /// 11: Font data is "lossless"
    /// 12: Font converted (produce compatible metrics)
    /// 13: Optimized for ClearType
    /// 14: Last Resort font
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_flags: Option<u16>,
    /// Head table lowest recommended pixels per em field
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_lowest_rec_ppem: Option<u16>,
    // OS/2 table
    /// OS/2 table `usWeightClass`` field
    ///
    /// Indicates the visual weight (degree of blackness or thickness of strokes) of the characters in the font. Values from 1 to 1000 are valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os2_us_weight_class: Option<u16>,
    /// OS/2 table `usWidthClass` field
    ///
    /// Indicates a relative change from the normal aspect ratio (width to height ratio) as specified by a font designer for the glyphs in a font.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os2_us_width_class: Option<u16>,
    /// OS/2 table `fsType` field
    ///
    /// A bit field that specifies font embedding and licensing rights for the font.
    /// The bits are:
    /// 0-3: Used to specify embedding permissions
    ///  A value of 0 indicates that the font may be embedded and installed permanently on the remote system.
    ///  A value of 2 indicates that the font may be embedded but must be uninstalled when the document is closed.
    ///  A value of 4 indicates that the font may be embedded for preview and printing only.
    ///  A value of 8 indicates that the font may not be embedded.
    /// 4-7: Reserved; set to 0
    /// 8: No subsetting
    /// 9: Bitmap embedding only
    /// 10-15: Reserved; set to 0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os2_fs_type: Option<u16>,

    // Subscript, Superscript X Size and Offset are Metrics
    /// Font-family class and subclass
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os2_family_class: Option<u16>,

    /// Panose classification
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os2_panose: Option<[u8; 10]>,

    /// Unicode range bits 0-31; see OpenType spec for bit meanings
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os2_unicode_range1: Option<u32>,
    /// Unicode range bits 32-63; see OpenType spec for bit meanings
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os2_unicode_range2: Option<u32>,
    /// Unicode range bits 64-95; see OpenType spec for bit meanings
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os2_unicode_range3: Option<u32>,
    /// Unicode range bits 96-127; see OpenType spec for bit meanings
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os2_unicode_range4: Option<u32>,
    /// OS/2 Vendor ID field
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[typeshare(typescript(type = "string | null"))]
    #[typeshare(python(type = "Optional[str]"))]
    pub os2_vendor_id: Option<Tag>,
    /// OS/2 fsSelection field
    ///
    /// A bit field. The bits are:
    /// 0: Italic
    /// 1: Underscore
    /// 2: Negative
    /// 3: Outlined
    /// 4: Strikeout
    /// 5: Bold
    /// 6: Regular
    /// 7: Use Typo Metrics
    /// 8: WWS
    /// 9: Oblique
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os2_fs_selection: Option<u16>,
    /// OS/2 Code Page Range 1 field
    /// A bit field; see OpenType spec for bit meanings
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os2_code_page_range1: Option<u32>,
    /// OS/2 Code Page Range 2 field
    /// A bit field; see OpenType spec for bit meanings
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os2_code_page_range2: Option<u32>,
    // CFF table
    /// CFF table BlueValues field
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cff_blue_values: Option<Vec<f64>>, // Probably not but it's what norad does
    /// CFF table OtherBlues field
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cff_other_blues: Option<Vec<f64>>,
    /// CFF table FamilyBlues field
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cff_family_blues: Option<Vec<f64>>,
    /// CFF table FamilyOtherBlues field
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cff_family_other_blues: Option<Vec<f64>>,
    /// CFF table StemSnapH field
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cff_stem_snap_h: Option<Vec<f64>>,
    /// CFF table StemSnapV field
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cff_stem_snap_v: Option<Vec<f64>>,
}

impl CustomOTValues {
    /// Check if there are any custom OpenType values set
    pub fn is_empty(&self) -> bool {
        self != &CustomOTValues::default()
    }
}
