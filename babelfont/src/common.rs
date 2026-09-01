use std::str::FromStr;

use crate::axis::Tag;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

/// Useful font-related constants
pub mod constants;
pub(crate) mod decomposition;
pub(crate) mod formatspecific;
mod node;
pub(crate) mod otvalue;
pub mod pathtools;
pub use node::{Node, NodeType};

use crate::BabelfontError;
pub use formatspecific::FormatSpecific;
pub use otvalue::CustomOTValues;

/// Split off the leading `1.002`-shaped number, returning it and whatever
/// trails it verbatim.
///
/// A trailing dot belongs to the text, not the number: "1." is read as "1"
/// followed by ".".
fn split_leading_version_number(value: &str) -> (&str, &str) {
    let end = value
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(value.len());
    let number = value[..end].trim_end_matches('.');
    (number, &value[number.len()..])
}

/// Split a version number into the (major, minor) pair `head.fontRevision` is
/// built from.
///
/// The compiler forms the revision as `format!("{major}.{minor:03}")`, so the
/// minor is the fractional DIGITS right-padded to three, not their numeric
/// value and not a float fraction: "1.002" is (1, 2) and "1.1" is (1, 100).
///
/// Computing it as `(frac * 100.0).round()` -- which is easy to reach for --
/// turns 1.002 into (1, 0), so `head.fontRevision` says 1.000 while name ID 5
/// says 1.002 and the two disagree.
pub(crate) fn version_major_minor(value: &str) -> Option<(u16, u16)> {
    let trimmed = value.trim().trim_start_matches("Version ").trim();
    // Only the number is ours; anything trailing it -- the ttfautohint
    // invocation FontForge writes into the same field, a year -- is text. Left
    // in, the fraction fails the digit test and the revision silently falls
    // back to X.000 while name ID 5 says X.YYY.
    let (trimmed, _) = split_leading_version_number(trimmed);
    let (major, fraction) = match trimmed.split_once('.') {
        Some((major, fraction)) => (major, fraction),
        None => (trimmed, ""),
    };
    if major.is_empty() || !major.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !fraction.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let major: u16 = major.parse().ok()?;
    let mut digits = fraction.to_string();
    while digits.len() < 3 {
        digits.push('0');
    }
    digits.truncate(3);
    Some((major, digits.parse().ok()?))
}

pub(crate) fn tag_from_string(s: &str) -> Result<Tag, BabelfontError> {
    if s.len() > 4 {
        return Err(BabelfontError::General(format!(
            "Tag must be 4 characters or less, got: '{}'",
            s
        )));
    }
    let mut chars = s.bytes().collect::<Vec<u8>>();
    while chars.len() < 4 {
        chars.push(b' ');
    }
    Ok(Tag::new(&chars[0..4].try_into().map_err(|_| {
        BabelfontError::General(format!("Bad tag: '{}'", s))
    })?))
}
#[derive(Debug, Copy, Clone, Default, Serialize, Deserialize, PartialEq)]
#[typeshare]
/// A position in 2D space, with an optional angle
pub struct Position {
    /// X coordinate
    pub x: f32,
    /// Y coordinate
    pub y: f32,
    /// Angle in degrees
    #[serde(default, skip_serializing_if = "crate::serde_helpers::is_zero")]
    pub angle: f32,
}

impl Position {
    /// Create a zeroed Position
    pub fn zero() -> Position {
        Position {
            x: 0.0,
            y: 0.0,
            angle: 0.0,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Copy, Clone, Default, Serialize, Deserialize, PartialEq)]
#[typeshare]
pub struct Color {
    pub r: i32,
    pub g: i32,
    pub b: i32,
    pub a: i32,
}

#[cfg(feature = "ufo")]
mod ufo {
    use super::*;
    impl From<&norad::Color> for Color {
        fn from(c: &norad::Color) -> Self {
            let (red, green, blue, alpha) = c.channels();
            Color {
                r: (red * 255.0) as i32,
                g: (green * 255.0) as i32,
                b: (blue * 255.0) as i32,
                a: (alpha * 255.0) as i32,
            }
        }
    }
    impl TryFrom<&Color> for norad::Color {
        type Error = BabelfontError;
        fn try_from(c: &Color) -> Result<Self, BabelfontError> {
            Ok(norad::Color::new(
                c.r as f64 / 255.0,
                c.g as f64 / 255.0,
                c.b as f64 / 255.0,
                c.a as f64 / 255.0,
            )?)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
#[typeshare]
/// Direction of text flow
pub enum Direction {
    /// Left to right text flow
    LeftToRight,
    /// Right to left text flow
    RightToLeft,
    /// Top to bottom text flow
    TopToBottom,
    /// Bidirectional,
    Bidi,
}

impl FromStr for Direction {
    type Err = BabelfontError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "lefttoright" | "ltr" => Ok(Direction::LeftToRight),
            "righttoleft" | "rtl" => Ok(Direction::RightToLeft),
            "toptobottom" | "ttb" | "vtr" => Ok(Direction::TopToBottom),
            "bidi" => Ok(Direction::Bidi),
            _ => Err(BabelfontError::General(format!(
                "Invalid direction string: {}",
                s
            ))),
        }
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod version_tests {
    use super::version_major_minor;

    #[test]
    fn the_minor_is_digits_not_a_float_fraction() {
        // The compiler builds the revision as "{major}.{minor:03}".
        assert_eq!(version_major_minor("1.002"), Some((1, 2)));
        assert_eq!(version_major_minor("1.1"), Some((1, 100)));
        assert_eq!(version_major_minor("2.32"), Some((2, 320)));
        assert_eq!(version_major_minor("Version 2.320"), Some((2, 320)));
        assert_eq!(version_major_minor("3"), Some((3, 0)));

        // Round-trip through the compiler's own format string.
        for (input, want) in [("1.002", "1.002"), ("1.1", "1.100"), ("2.32", "2.320")] {
            let (major, minor) = version_major_minor(input).unwrap();
            assert_eq!(format!("{major}.{minor:03}"), want);
        }

        // A float fraction is the wrong model: (1.002 - 1.0) * 100.0 rounds
        // to 0, collapsing the minor while name ID 5 still says 1.002.
        let (_, minor) = version_major_minor("1.002").unwrap();
        assert_ne!(minor, 0, "1.002 must not collapse to a zero minor");

        assert_eq!(version_major_minor("one.two"), None);
        assert_eq!(version_major_minor(""), None);
    }

    #[test]
    fn text_after_the_number_is_not_parsed_as_part_of_it() {
        // FontForge writes the ttfautohint invocation into the same field.
        let hinted = "1.001; ttfautohint (v0.92) -l 10 -r 16 -G 200 -x 7 -w \"GD\"";
        assert_eq!(version_major_minor(hinted), Some((1, 1)));
        assert_eq!(version_major_minor("1.002 2010"), Some((1, 2)));
        assert_eq!(
            version_major_minor("Version 1.001; ttfautohint"),
            Some((1, 1))
        );
        // A trailing dot belongs to the text, not the number.
        assert_eq!(version_major_minor("1.002. Released 2010"), Some((1, 2)));
    }
}
