use crate::filters::fontforge_one_width::one_width;
use crate::filters::FontFilter;
use crate::{Font, Glyph, Layer, LayerType, Master, MetricType, Node, NodeType, Path, Shape};

#[derive(Default)]
/// A filter that gives a FontForge source the `.notdef` FontForge's TrueType exporter
/// writes when the source has none.
///
/// Glyph 0 of a FontForge TrueType export is the source glyph named `.notdef`. When the
/// source has no glyph of that name, the exporter writes a hollow rectangle of its own,
/// so the binary has a `.notdef` that is nowhere in the source. In a fixed-pitch font
/// (every glyph that counts shares one advance) a `.notdef` whose advance differs from
/// that shared advance is replaced by the rectangle too.
///
/// The rectangle, in font units, with `em` the source's ascent + descent:
///
/// ```text
/// stem    = StdVW from the private dictionary, else StdHW, else em/30
/// ymax    = min(2*em/3, ascent)
/// xmax    = 5*stem + em/10 + stem
/// outer   = (stem,0) (stem,ymax) (xmax,ymax) (xmax,0)
/// inner   = (2*stem,stem) (xmax-stem,stem) (xmax-stem,ymax-stem) (2*stem,ymax-stem)
/// advance = the fixed pitch, else xmax + 2*stem
/// ```
///
/// All divisions are integer divisions. `StdVW` and `StdHW` count only when their
/// value starts with a positive number: the bracketed array a private dictionary
/// normally holds (`[85]`) reads as 0, so the stem is then `em/30`.
///
/// The outer contour runs clockwise and the inner one counter-clockwise, as in the
/// exported binary. The glyph is inserted first in the glyph order.
///
/// # Opt-in, because it is a statement about who exported the binary
///
/// A source without a `.notdef` is not wrong: every compiler supplies one. This
/// filter only makes sense when reproducing a binary FontForge exported.
pub struct FontForgeNotdef;

impl FontForgeNotdef {
    /// Create a new FontForgeNotdef filter
    pub fn new() -> Self {
        FontForgeNotdef
    }
}

/// The outline and advance of the exported `.notdef` for one master.
struct MissingGlyph {
    outer: [(i32, i32); 4],
    inner: [(i32, i32); 4],
    advance: i32,
}

impl MissingGlyph {
    fn new(ascent: i32, descent: i32, private_stem: Option<i32>, fixed_pitch: Option<i32>) -> Self {
        let em = ascent + descent;
        let stem = private_stem.filter(|s| *s > 0).unwrap_or(em / 30);
        let ymax = (2 * em / 3).min(ascent);
        let xmax = 5 * stem + em / 10 + stem;
        MissingGlyph {
            outer: [(stem, 0), (stem, ymax), (xmax, ymax), (xmax, 0)],
            inner: [
                (2 * stem, stem),
                (xmax - stem, stem),
                (xmax - stem, ymax - stem),
                (2 * stem, ymax - stem),
            ],
            advance: fixed_pitch.unwrap_or(xmax + 2 * stem),
        }
    }

    fn layer(&self, master: &Master) -> Layer {
        let path = |points: &[(i32, i32); 4]| {
            Shape::Path(Path {
                nodes: points
                    .iter()
                    .map(|&(x, y)| Node {
                        x: f64::from(x),
                        y: f64::from(y),
                        nodetype: NodeType::Line,
                        ..Default::default()
                    })
                    .collect(),
                closed: true,
                ..Default::default()
            })
        };
        let mut layer = Layer::new(self.advance as f32);
        layer.master = LayerType::DefaultForMaster(master.id.clone());
        layer.shapes = vec![path(&self.outer), path(&self.inner)];
        layer
    }
}

/// The stem the private dictionary states: `StdVW`, else `StdHW`, each read as a
/// leading number truncated to an integer, so a bracketed array reads as 0.
fn private_stem(font: &Font) -> Option<i32> {
    let lines: Vec<&str> = font
        .format_specific
        .get("sfd.private_section")?
        .as_array()?
        .iter()
        .filter_map(|l| l.as_str())
        .collect();
    let entry = |key: &str| -> Option<i32> {
        lines.iter().find_map(|line| {
            let mut fields = line.trim().splitn(3, ' ');
            if fields.next() != Some(key) {
                return None;
            }
            let _length = fields.next();
            Some(leading_number(fields.next().unwrap_or("")))
        })
    };
    // StdHW is consulted only when there is no StdVW entry at all.
    entry("StdVW").or_else(|| entry("StdHW"))
}

/// The number at the start of `text`, truncated towards zero; 0 when it does not
/// start with one.
fn leading_number(text: &str) -> i32 {
    let text = text.trim_start();
    let end = text
        .char_indices()
        .find(|&(i, c)| {
            !(c.is_ascii_digit() || c == '.' || ((c == '-' || c == '+') && i == 0))
        })
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    text[..end].parse::<f64>().map(|v| v as i32).unwrap_or(0)
}

impl FontFilter for FontForgeNotdef {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let stem = private_stem(font);
        let existing = font.glyphs.iter().position(|g| g.name == ".notdef");
        let mut layers = Vec::new();
        for master in font.masters.iter() {
            let ascender = master.metrics.get(&MetricType::Ascender).copied();
            let descender = master.metrics.get(&MetricType::Descender).copied();
            let (Some(ascent), Some(descender)) = (ascender, descender) else {
                log::warn!(
                    "Master {} states no ascender and descender; no .notdef synthesized",
                    master.id
                );
                return Ok(());
            };
            let fixed_pitch = one_width(font, master).filter(|w| *w > 0);
            if let Some(index) = existing {
                // Kept unless the font is fixed pitch and its advance differs.
                let Some(pitch) = fixed_pitch else {
                    return Ok(());
                };
                let advance = font
                    .master_layer_for(".notdef", master)
                    .map(|layer| layer.width.round() as i32);
                if advance == Some(pitch) {
                    return Ok(());
                }
                log::debug!("Replacing .notdef at glyph index {index}");
            }
            // babelfont's descender is negative below the baseline.
            let missing = MissingGlyph::new(ascent, -descender, stem, fixed_pitch);
            layers.push(missing.layer(master));
        }
        if let Some(index) = existing {
            font.glyphs.remove(index);
        }
        let mut notdef = Glyph::new(".notdef");
        notdef.exported = true;
        notdef.layers = layers;
        font.glyphs.insert(0, notdef);
        log::info!(
            "{} .notdef with the hollow rectangle FontForge's TrueType exporter writes",
            if existing.is_some() {
                "Replaced the fixed-pitch font's"
            } else {
                "Added a"
            }
        );
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(FontForgeNotdef::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("fontforgenotdef")
            .long("fontforge-notdef")
            .help(
                "Give a FontForge source that has no .notdef the hollow rectangle FontForge's \
                 TrueType exporter writes in its place (and replace a .notdef whose advance \
                 differs from a fixed-pitch font's shared advance). Use only to reproduce a \
                 binary FontForge exported",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    fn font_with(ascender: i32, descender: i32) -> Font {
        let mut m = Master::default();
        m.metrics.insert(MetricType::Ascender, ascender);
        m.metrics.insert(MetricType::Descender, descender);
        let mut f = Font::new();
        f.masters.push(m);
        f
    }

    /// A glyph with one triangular contour (or none) and the given advance.
    fn add_glyph(f: &mut Font, name: &str, advance: f32, contour: bool) {
        let mut layer = Layer::new(advance);
        layer.master = LayerType::DefaultForMaster(f.masters[0].id.clone());
        if contour {
            let node = |x: f64, y: f64| Node {
                x,
                y,
                nodetype: NodeType::Line,
                ..Default::default()
            };
            layer.shapes.push(Shape::Path(Path {
                nodes: vec![node(0.0, 0.0), node(100.0, 0.0), node(50.0, 100.0)],
                closed: true,
                ..Default::default()
            }));
        }
        let mut glyph = Glyph::new(name);
        glyph.layers.push(layer);
        f.glyphs.push(glyph);
    }

    fn points(f: &Font) -> Vec<Vec<(f64, f64)>> {
        f.glyphs[0].layers[0]
            .shapes
            .iter()
            .map(|s| match s {
                Shape::Path(p) => p.nodes.iter().map(|n| (n.x, n.y)).collect(),
                Shape::Component(_) => vec![],
            })
            .collect()
    }

    fn set_private(f: &mut Font, lines: &[&str]) {
        f.format_specific.insert(
            "sfd.private_section".to_string(),
            serde_json::Value::Array(
                lines
                    .iter()
                    .map(|l| serde_json::Value::String(l.to_string()))
                    .collect(),
            ),
        );
    }

    #[test]
    fn test_a_2048_em_gets_the_748_unit_rectangle() {
        // Ascent 1638 + descent 410: stem 68, ymax 1365, xmax 612, advance 748.
        let mut f = font_with(1638, -410);
        add_glyph(&mut f, "a", 600.0, true);
        add_glyph(&mut f, "b", 500.0, true);
        FontForgeNotdef::new().apply(&mut f).unwrap();
        assert_eq!(f.glyphs[0].name, ".notdef");
        assert_eq!(f.glyphs[0].layers[0].width, 748.0);
        assert_eq!(
            points(&f),
            vec![
                vec![(68.0, 0.0), (68.0, 1365.0), (612.0, 1365.0), (612.0, 0.0)],
                vec![
                    (136.0, 68.0),
                    (544.0, 68.0),
                    (544.0, 1297.0),
                    (136.0, 1297.0)
                ],
            ]
        );
        assert_eq!(f.glyphs[1].name, "a");
        assert!(f.glyphs[0].exported);
    }

    #[test]
    fn test_a_1024_em_gets_the_374_unit_rectangle() {
        // 819 + 205: stem 34, ymax 682, xmax 306, advance 374.
        let mut f = font_with(819, -205);
        add_glyph(&mut f, "a", 600.0, true);
        add_glyph(&mut f, "b", 500.0, true);
        FontForgeNotdef::new().apply(&mut f).unwrap();
        assert_eq!(f.glyphs[0].layers[0].width, 374.0);
        assert_eq!(points(&f)[0][2], (306.0, 682.0));
    }

    #[test]
    fn test_the_height_is_capped_at_the_ascent() {
        // 2*1000/3 = 666 exceeds an ascent of 600.
        let mut f = font_with(600, -400);
        add_glyph(&mut f, "a", 600.0, true);
        add_glyph(&mut f, "b", 500.0, true);
        FontForgeNotdef::new().apply(&mut f).unwrap();
        assert_eq!(points(&f)[0][1], (33.0, 600.0));
    }

    #[test]
    fn test_a_bracketed_stem_reads_as_zero_and_a_plain_one_counts() {
        let mut f = font_with(1638, -410);
        add_glyph(&mut f, "a", 600.0, true);
        add_glyph(&mut f, "b", 500.0, true);
        set_private(&mut f, &["BlueValues 23 [-20 0 1000 1020]", "StdVW 4 [85]"]);
        FontForgeNotdef::new().apply(&mut f).unwrap();
        assert_eq!(f.glyphs[0].layers[0].width, 748.0);

        // A plain number is the stem: xmax = 5*85 + 204 + 85 = 714, advance 884.
        let mut f = font_with(1638, -410);
        add_glyph(&mut f, "a", 600.0, true);
        add_glyph(&mut f, "b", 500.0, true);
        set_private(&mut f, &["StdVW 2 85"]);
        FontForgeNotdef::new().apply(&mut f).unwrap();
        assert_eq!(f.glyphs[0].layers[0].width, 884.0);
        assert_eq!(points(&f)[0][0], (85.0, 0.0));

        // StdHW is read only when there is no StdVW entry.
        let mut f = font_with(1638, -410);
        add_glyph(&mut f, "a", 600.0, true);
        add_glyph(&mut f, "b", 500.0, true);
        set_private(&mut f, &["StdHW 2 85"]);
        FontForgeNotdef::new().apply(&mut f).unwrap();
        assert_eq!(f.glyphs[0].layers[0].width, 884.0);
    }

    #[test]
    fn test_a_source_notdef_is_kept() {
        // Even an empty one: the exported glyph 0 is the source's .notdef.
        let mut f = font_with(1638, -410);
        add_glyph(&mut f, "a", 600.0, true);
        add_glyph(&mut f, "b", 500.0, true);
        add_glyph(&mut f, ".notdef", 500.0, false);
        let before = f.glyphs.clone();
        FontForgeNotdef::new().apply(&mut f).unwrap();
        assert_eq!(f.glyphs, before);
    }

    #[test]
    fn test_a_fixed_pitch_font_gets_its_pitch_as_the_advance() {
        let mut f = font_with(1638, -410);
        add_glyph(&mut f, "a", 1229.0, true);
        add_glyph(&mut f, "b", 1229.0, true);
        FontForgeNotdef::new().apply(&mut f).unwrap();
        assert_eq!(f.glyphs[0].layers[0].width, 1229.0);
        assert_eq!(points(&f)[0][2], (612.0, 1365.0));
    }

    #[test]
    fn test_a_fixed_pitch_font_replaces_an_empty_notdef_of_another_advance() {
        let mut f = font_with(1638, -410);
        add_glyph(&mut f, "a", 1229.0, true);
        add_glyph(&mut f, "b", 1229.0, true);
        add_glyph(&mut f, ".notdef", 500.0, false);
        FontForgeNotdef::new().apply(&mut f).unwrap();
        assert_eq!(f.glyphs.len(), 3);
        assert_eq!(f.glyphs[0].name, ".notdef");
        assert_eq!(f.glyphs[0].layers[0].width, 1229.0);
        assert_eq!(points(&f).len(), 2);

        // One of the same advance is kept.
        let mut f = font_with(1638, -410);
        add_glyph(&mut f, "a", 1229.0, true);
        add_glyph(&mut f, ".notdef", 1229.0, false);
        let before = f.glyphs.clone();
        FontForgeNotdef::new().apply(&mut f).unwrap();
        assert_eq!(f.glyphs, before);
    }
}
