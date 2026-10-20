//! Evaluation of Glyphs glyph-class predicates such as `$[name endswith ".sc"]`.
//!
//! This is a port of the glyph predicate part of glyphsLib's `TokenExpander`.
//! Glyphs-specific glyph and layer data is read from `format_specific`, so it
//! is only available for fonts which came from a Glyphs source.

// The regular expressions are compile-time constants, so they cannot fail to build.
#![allow(clippy::unwrap_used)]

use std::{collections::HashSet, sync::LazyLock};

use indexmap::IndexSet;
use regex::Regex;
use serde_json::Value as Json;
use smol_str::SmolStr;

use crate::filters::FontFilter;
use crate::{BabelfontError, Direction, Font, Glyph, GlyphCategory, Shape};

/// A filter that replaces Glyphs 3 glyph predicates (`$[name endswith ".sc"]`)
/// in class definitions, feature prefixes and features with the glyphs they select
#[derive(Default)]
pub struct ExpandGlyphs3Predicates;

impl ExpandGlyphs3Predicates {
    /// Create a new ExpandGlyphs3Predicates filter
    pub fn new() -> Self {
        ExpandGlyphs3Predicates
    }
}

impl FontFilter for ExpandGlyphs3Predicates {
    fn apply(&self, font: &mut Font) -> Result<(), BabelfontError> {
        log::info!("Expanding Glyphs 3 predicates in feature code");
        // Work on a copy so that a failure leaves the font untouched
        let mut features = font.features.clone();
        expand_features(font, &mut features)?;
        font.features = features;
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, BabelfontError>
    where
        Self: Sized,
    {
        Ok(ExpandGlyphs3Predicates::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("expandglyphs3predicates")
            .long("expand-glyphs3-predicates")
            .help("Expand Glyphs 3 glyph predicates ($[...]) in feature code into glyph lists")
            .action(clap::ArgAction::SetTrue)
    }
}

fn expand_features(font: &Font, features: &mut crate::Features) -> Result<(), BabelfontError> {
    // Class code is already inside a glyph class literal
    for class in features.classes.values_mut() {
        class.code = expand_predicates_in_fea(font, &class.code, PredicateOutput::Bare)?;
    }
    for prefix in features.prefixes.values_mut() {
        prefix.code = expand_predicates_in_fea(font, &prefix.code, PredicateOutput::Class)?;
    }
    for (_, feature) in features.features.iter_mut() {
        feature.code = expand_predicates_in_fea(font, &feature.code, PredicateOutput::Class)?;
    }
    Ok(())
}
// Keys under which the Glyphs loader stores layer data (see `convertors::glyphs3`).
const KEY_ATTR: &str = "com.schriftgestalt.Glyphs.attr";
const KEY_ANNOTATIONS: &str = "com.schriftgestalt.Glyphs.annotations";
const KEY_LAYER_HINTS: &str = "com.schriftgestalt.Glyphs.layerHints";

const PREDICATE_OBJECTS: &[&str] = &[
    "hasAlignedWidth",
    "hasAnnotations",
    "hasComponents",
    "hasCorners",
    "hasCustomGlyphInfo",
    "hasHints",
    "hasPostScriptHints",
    "hasSpecialLayers",
    "hasTrueTypeHints",
    "isAligned",
    "isAppleColorGlyph",
    "isAnyColorGlyph",
    "isFullColorGlyph",
    "isCornerGlyph",
    "isHangulKeyGlyph",
    "isSVGColorGlyph",
    "isColorPaletteGlyph",
    "isSmartGlyph",
    "justLocked",
    "locked",
    "mastersCompatible",
    "outlineHasChanges",
    "case",
    "changeCount",
    "colorIndex",
    "countOfLayers",
    "countOfPartsSettings",
    "countOfTags",
    "countOfUnicodes",
    "direction",
    "baseString",
    "bottomKerningGroup",
    "bottomMetricsKey",
    "category",
    "charName",
    "charString",
    "description",
    "glyphDataEntryString",
    "lastChange",
    "leftKerningGroup",
    "leftKerningKey",
    "leftMetricsKey",
    "name",
    "note",
    "production",
    "productionName",
    "rightKerningGroup",
    "rightKerningKey",
    "rightMetricsKey",
    "script",
    "sortName",
    "sortNameKeep",
    "string",
    "subCategory",
    "tags",
    "topKerningGroup",
    "topMetricsKey",
    "unicode",
    "unicodeChar",
    "unicodeString",
    "vertWidthMetricsKey",
    "widthMetricsKey",
];

static COMPARATOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(beginswith|contains|endswith|like|matches|==?|>=|=>|<=|=<|!=|<>|>|<|between|in)\s*",
    )
    .unwrap()
});
static NOT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\s*(not|!)\s+").unwrap());
static IDENT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(\w+)").unwrap());
static IN_WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bin\b").unwrap());
static DQUOTED_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*"([^"]+)""#).unwrap());
static SQUOTED_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*'([^']+)'").unwrap());
static TRUE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\s*(yes|true)\b").unwrap());
static FALSE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\s*(no|false)\b").unwrap());
static INT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(\d+)").unwrap());
static AND_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\s*(and\b|&&)").unwrap());
static OR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\s*(or\b|\|\|)").unwrap());
static OPEN_BRACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*\{").unwrap());
static CLOSE_BRACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*\}").unwrap());
static COMMA_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*,").unwrap());

/// Resolve a Glyphs predicate class (`$[...]`) to the names of the exported
/// glyphs which satisfy it.
///
/// A predicate which cannot be parsed, or which uses an unsupported property,
/// is logged and resolves to no glyphs. Use
/// [`try_resolve_glyphs_predicate_class`] to get the error instead.
pub(crate) fn resolve_glyphs_predicate_class(font: &Font, predicate: &str) -> Vec<SmolStr> {
    try_resolve_glyphs_predicate_class(font, predicate).unwrap_or_else(|e| {
        log::warn!("Could not resolve glyph predicate '{predicate}': {e}");
        vec![]
    })
}

/// As [`resolve_glyphs_predicate_class`], but reporting errors.
///
/// The predicate may be given with or without its `$[` and `]` delimiters.
pub(crate) fn try_resolve_glyphs_predicate_class(
    font: &Font,
    predicate: &str,
) -> Result<Vec<SmolStr>, BabelfontError> {
    let trimmed = predicate.trim();
    let inner = trimmed
        .strip_prefix("$[")
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(trimmed);
    let mut parser = PredicateParser {
        font,
        original: inner,
        rest: inner,
    };
    let glyphs = parser.parse_to_array()?;
    if !parser.rest.trim().is_empty() {
        return Err(parser.error("end of predicate"));
    }
    Ok(glyphs)
}

/// How an expanded predicate is written back into feature code
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PredicateOutput {
    /// A literal glyph class: `[a b c]`
    Class,
    /// Bare space-separated glyph names, for use where the surrounding code is
    /// already a glyph class literal
    Bare,
}

/// Replace every `$[...]` predicate in a piece of feature code with the glyphs
/// it selects.
///
/// This works on the text rather than a parsed feature file so that predicates
/// inside class definitions, which don't round-trip through an AST, are handled
/// the same way as those in feature code. Comments and quoted strings outside of
/// predicates are left alone, and the `]` which closes a predicate is not
/// confused by one inside a quoted value.
pub(crate) fn expand_predicates_in_fea(
    font: &Font,
    code: &str,
    output: PredicateOutput,
) -> Result<String, BabelfontError> {
    let mut result = String::with_capacity(code.len());
    let mut rest = code;
    while let Some(i) = rest.find(['#', '"', '$']) {
        result.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = match rest.as_bytes()[0] {
            b'#' => rest.find('\n').unwrap_or(rest.len()),
            b'"' => rest[1..].find('"').map_or(rest.len(), |e| e + 2),
            _ if rest.starts_with("$[") => match predicate_end(rest) {
                Some(end) => {
                    let glyphs = try_resolve_glyphs_predicate_class(font, &rest[..end])?;
                    let names = glyphs.iter().map(|g| g.as_str()).collect::<Vec<_>>();
                    match output {
                        PredicateOutput::Class => {
                            result.push('[');
                            result.push_str(&names.join(" "));
                            result.push(']');
                        }
                        PredicateOutput::Bare => result.push_str(&names.join(" ")),
                    }
                    rest = &rest[end..];
                    continue;
                }
                None => {
                    return Err(BabelfontError::FilterError(format!(
                        "Unterminated glyph predicate '{}'",
                        rest.lines().next().unwrap_or(rest)
                    )))
                }
            },
            _ => 1,
        };
        result.push_str(&rest[..end]);
        rest = &rest[end..];
    }
    result.push_str(rest);
    Ok(result)
}

/// The byte offset just past the `]` closing the predicate at the start of `s`
fn predicate_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices().skip(2) {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, ']') => return Some(i + 1),
            _ => {}
        }
    }
    None
}

/// A value on either side of a comparison
#[derive(Debug, Clone, PartialEq)]
enum Value {
    None,
    Bool(bool),
    Int(i64),
    Str(String),
    List(Vec<String>),
}

impl Value {
    /// Python's `str()` of the value, which is what string comparators see.
    fn as_text(&self) -> String {
        match self {
            Value::None => "None".to_string(),
            Value::Bool(true) => "True".to_string(),
            Value::Bool(false) => "False".to_string(),
            Value::Int(i) => i.to_string(),
            Value::Str(s) => s.clone(),
            Value::List(l) => format!(
                "[{}]",
                l.iter()
                    .map(|s| format!("'{s}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    fn as_number(&self) -> Option<i64> {
        match self {
            Value::Bool(b) => Some(*b as i64),
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    fn loosely_equals(&self, other: &Value) -> bool {
        match (self.as_number(), other.as_number()) {
            (Some(a), Some(b)) => a == b,
            _ => self == other,
        }
    }

    fn compare(&self, other: &Value) -> Option<std::cmp::Ordering> {
        match (self.as_number(), other.as_number()) {
            (Some(a), Some(b)) => Some(a.cmp(&b)),
            _ => match (self, other) {
                (Value::Str(a), Value::Str(b)) => Some(a.cmp(b)),
                _ => None,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Comparator {
    BeginsWith,
    Contains,
    EndsWith,
    Like,
    Matches,
    Eq,
    Ne,
    Ge,
    Le,
    Gt,
    Lt,
    Between,
    In,
}

impl Comparator {
    fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "beginswith" => Self::BeginsWith,
            "contains" => Self::Contains,
            "endswith" => Self::EndsWith,
            "like" => Self::Like,
            "matches" => Self::Matches,
            "=" | "==" => Self::Eq,
            "!=" | "<>" => Self::Ne,
            ">=" | "=>" => Self::Ge,
            "<=" | "=<" => Self::Le,
            ">" => Self::Gt,
            "<" => Self::Lt,
            "between" => Self::Between,
            _ => Self::In,
        }
    }
}

/// What a comparator is applied against: a single value or an aggregate `{...}`
enum Expected {
    One(Value),
    Many(Vec<Value>),
}

struct PredicateParser<'a> {
    font: &'a Font,
    original: &'a str,
    rest: &'a str,
}

impl<'a> PredicateParser<'a> {
    fn error(&self, thing: &str) -> BabelfontError {
        let found = self.rest.split_whitespace().next().unwrap_or("");
        BabelfontError::FilterError(format!(
            "Expected {thing}, found '{found}' in predicate '{}'",
            self.original
        ))
    }

    /// Consume a match of `re` against the start of the remaining text
    fn eat(&mut self, re: &Regex) -> Option<regex::Captures<'a>> {
        let rest: &'a str = self.rest;
        let caps = re.captures(rest)?;
        self.rest = &rest[caps.get(0).unwrap().end()..];
        Some(caps)
    }

    fn parse_optional_not(&mut self) -> bool {
        self.eat(&NOT_RE).is_some()
    }

    fn parse_object(&mut self) -> Result<&'static str, BabelfontError> {
        let found = IDENT_RE
            .captures(self.rest)
            .and_then(|c| PREDICATE_OBJECTS.iter().find(|o| **o == &c[1]));
        match found {
            Some(object) => {
                self.eat(&IDENT_RE);
                Ok(object)
            }
            None => Err(self.error("known glyph predicate object")),
        }
    }

    fn parse_comparator(&mut self) -> Result<Comparator, BabelfontError> {
        match self.eat(&COMPARATOR_RE) {
            Some(caps) => Ok(Comparator::parse(&caps[1])),
            None => Err(self.error("comparator")),
        }
    }

    fn parse_value(&mut self) -> Result<Value, BabelfontError> {
        if let Some(c) = self.eat(&DQUOTED_RE).or_else(|| self.eat(&SQUOTED_RE)) {
            return Ok(Value::Str(c[1].to_string()));
        }
        if self.eat(&TRUE_RE).is_some() {
            return Ok(Value::Bool(true));
        }
        if self.eat(&FALSE_RE).is_some() {
            return Ok(Value::Bool(false));
        }
        if let Some(c) = self.eat(&INT_RE) {
            return c[1]
                .parse()
                .map(Value::Int)
                .map_err(|_| self.error("value"));
        }
        // Keyword constants
        if let Some(c) = self.eat(&IDENT_RE) {
            return Ok(Value::Str(c[1].to_string()));
        }
        Err(self.error("value"))
    }

    fn parse_aggregate(&mut self) -> Result<Vec<Value>, BabelfontError> {
        if self.eat(&OPEN_BRACE_RE).is_none() {
            return Err(self.error("aggregate '{...}'"));
        }
        let mut values = vec![self.parse_value()?];
        while self.eat(&COMMA_RE).is_some() {
            values.push(self.parse_value()?);
        }
        if self.eat(&CLOSE_BRACE_RE).is_none() {
            return Err(self.error("'}'"));
        }
        Ok(values)
    }

    /// Parse a single clause plus anything it is joined to with `and` / `or`.
    fn parse_to_array(&mut self) -> Result<Vec<SmolStr>, BabelfontError> {
        let invert = self.parse_optional_not();
        let object;
        let comparator;
        let expected;
        if IDENT_RE
            .captures(self.rest)
            .is_some_and(|c| PREDICATE_OBJECTS.contains(&&c[1]))
        {
            object = self.parse_object()?;
            comparator = self.parse_comparator()?;
            expected = if matches!(comparator, Comparator::In | Comparator::Between) {
                Expected::Many(self.parse_aggregate()?)
            } else {
                Expected::One(self.parse_value()?)
            };
        } else if IN_WORD_RE.is_match(self.rest) {
            // `value in property` is `property contains value`
            let value = self.parse_value()?;
            if self.parse_comparator()? != Comparator::In {
                return Err(self.error("'in'"));
            }
            object = self.parse_object()?;
            comparator = Comparator::Contains;
            expected = Expected::One(value);
        } else {
            return Err(self.error("known glyph predicate object"));
        }

        let mut glyphs: IndexSet<SmolStr> = IndexSet::new();
        for g in self.font.glyphs.iter().filter(|g| g.exported) {
            let got = glyph_value(g, object).ok_or_else(|| {
                BabelfontError::FilterError(format!(
                    "Glyphs attribute {object} used in predicate '{}' but it is not supported",
                    self.original
                ))
            })?;
            let mut truth = compare(&got, comparator, &expected)?;
            if invert {
                truth = !truth;
            }
            if truth {
                glyphs.insert(g.name.clone());
            }
        }

        if self.eat(&AND_RE).is_some() {
            let other: HashSet<SmolStr> = self.parse_to_array()?.into_iter().collect();
            return Ok(glyphs.into_iter().filter(|g| other.contains(g)).collect());
        }
        if self.eat(&OR_RE).is_some() {
            glyphs.extend(self.parse_to_array()?);
        }
        Ok(glyphs.into_iter().collect())
    }
}

fn compare(
    got: &Value,
    comparator: Comparator,
    expected: &Expected,
) -> Result<bool, BabelfontError> {
    use std::cmp::Ordering::*;
    let one = match (comparator, expected) {
        (Comparator::In, Expected::Many(values)) => {
            return Ok(values.iter().any(|v| got.loosely_equals(v)))
        }
        (Comparator::Between, Expected::Many(values)) => {
            return Ok(match (values.first(), values.get(1)) {
                (Some(lo), Some(hi)) => {
                    matches!(got.compare(lo), Some(Greater | Equal))
                        && matches!(got.compare(hi), Some(Less | Equal))
                }
                _ => false,
            })
        }
        (_, Expected::One(v)) => v,
        _ => return Ok(false),
    };
    Ok(match comparator {
        Comparator::BeginsWith => got.as_text().starts_with(&one.as_text()),
        Comparator::EndsWith => got.as_text().ends_with(&one.as_text()),
        Comparator::Contains => match got {
            Value::List(items) => items.iter().any(|i| *i == one.as_text()),
            _ => got.as_text().contains(&one.as_text()),
        },
        Comparator::Like => fnmatch(&one.as_text(), &got.as_text()),
        Comparator::Matches => Regex::new(&one.as_text())
            .map_err(|e| BabelfontError::FilterError(format!("Invalid regular expression: {e}")))?
            .is_match(&got.as_text()),
        Comparator::Eq => got.loosely_equals(one),
        Comparator::Ne => !got.loosely_equals(one),
        Comparator::Ge => matches!(got.compare(one), Some(Greater | Equal)),
        Comparator::Le => matches!(got.compare(one), Some(Less | Equal)),
        Comparator::Gt => matches!(got.compare(one), Some(Greater)),
        Comparator::Lt => matches!(got.compare(one), Some(Less)),
        Comparator::Between | Comparator::In => false,
    })
}

/// Case-sensitive Unix shell-style wildcard match (`*`, `?`, `[seq]`, `[!seq]`)
fn fnmatch(pattern: &str, text: &str) -> bool {
    let mut re = String::from("(?s)^");
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => re.push_str(".*"),
            '?' => re.push('.'),
            '[' => {
                let mut j = i + 1;
                if j < chars.len() && chars[j] == '!' {
                    j += 1;
                }
                if j < chars.len() && chars[j] == ']' {
                    j += 1;
                }
                while j < chars.len() && chars[j] != ']' {
                    j += 1;
                }
                if j >= chars.len() {
                    re.push_str("\\[");
                } else {
                    let body: String = chars[i + 1..j].iter().collect();
                    let body = match body.strip_prefix('!') {
                        Some(rest) => format!("^{}", rest.replace('\\', "\\\\")),
                        None => body.replace('\\', "\\\\"),
                    };
                    re.push('[');
                    re.push_str(&body);
                    re.push(']');
                    i = j;
                }
            }
            c => re.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    re.push('$');
    Regex::new(&re).is_ok_and(|r| r.is_match(text))
}

fn layer_attr<'a>(layer: &'a crate::Layer, key: &str) -> Option<&'a Json> {
    layer.format_specific.get(KEY_ATTR).and_then(|a| a.get(key))
}

fn glyph_str(g: &Glyph, key: &str) -> Value {
    g.format_specific
        .get_optionstring(key)
        .map_or(Value::None, Value::Str)
}

fn layer_hints(layer: &crate::Layer) -> &[Json] {
    layer
        .format_specific
        .get(KEY_LAYER_HINTS)
        .and_then(|h| h.as_array())
        .map_or(&[], |v| v.as_slice())
}

fn has_hint_type(g: &Glyph, wanted: &[&str]) -> bool {
    g.layers.iter().any(|l| {
        layer_hints(l).iter().any(|h| {
            h.get("type")
                .and_then(|t| t.as_str())
                .is_some_and(|t| wanted.contains(&t))
        })
    })
}

fn has_attr(g: &Glyph, keys: &[&str]) -> bool {
    g.layers
        .iter()
        .filter(|l| !l.is_background)
        .any(|l| keys.iter().any(|k| layer_attr(l, k).is_some()))
}

fn tags(g: &Glyph) -> Vec<String> {
    g.format_specific
        .get("tags")
        .and_then(|t| t.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// The value of a predicate object for a glyph, or `None` if we can't
/// determine it (because babelfont doesn't carry the information).
fn glyph_value(g: &Glyph, object: &str) -> Option<Value> {
    let layers = || g.layers.iter().filter(|l| !l.is_background);
    let fs = &g.format_specific;
    Some(match object {
        "name" => Value::Str(g.name.to_string()),
        "production" | "productionName" => {
            Value::Str(g.production_name.as_ref().unwrap_or(&g.name).to_string())
        }
        "category" => match &g.category {
            GlyphCategory::Unknown => Value::None,
            other => Option::<String>::from(other).map_or(Value::None, Value::Str),
        },
        "subCategory" => glyph_str(g, "subcategory"),
        "script" => glyph_str(g, "script"),
        "note" => glyph_str(g, "note"),
        "case" => match fs.get("case") {
            Some(Json::Number(n)) => Value::Int(n.as_i64()?),
            Some(Json::String(s)) => Value::Str(s.clone()),
            _ => Value::None,
        },
        "colorIndex" => match fs.get("color") {
            Some(Json::Number(n)) => Value::Int(n.as_i64()?),
            _ => Value::None,
        },
        "lastChange" => glyph_str(g, "last_change"),
        "sortName" => glyph_str(g, "sort_name"),
        "sortNameKeep" => glyph_str(g, "sort_name_keep"),
        "leftKerningGroup" => glyph_str(g, "kern_left"),
        "rightKerningGroup" => glyph_str(g, "kern_right"),
        "topKerningGroup" => glyph_str(g, "kern_top"),
        "bottomKerningGroup" => glyph_str(g, "kern_bottom"),
        "leftMetricsKey" => glyph_str(g, "metric_left"),
        "rightMetricsKey" => glyph_str(g, "metric_right"),
        "topMetricsKey" => glyph_str(g, "metric_top"),
        "bottomMetricsKey" => glyph_str(g, "metric_bottom"),
        "widthMetricsKey" => glyph_str(g, "metric_width"),
        "vertWidthMetricsKey" => glyph_str(g, "metric_vert_width"),
        "tags" => Value::List(tags(g)),
        "countOfTags" => Value::Int(tags(g).len() as i64),
        "unicode" => Value::Str(
            g.codepoints
                .iter()
                .map(|c| format!("{c:04X}"))
                .collect::<Vec<_>>()
                .join(","),
        ),
        "unicodeChar" | "string" | "charString" => Value::Str(
            g.codepoints
                .first()
                .and_then(|c| char::from_u32(*c))
                .map(String::from)
                .unwrap_or_default(),
        ),
        "unicodeString" => Value::Str(
            g.codepoints
                .iter()
                .filter_map(|c| char::from_u32(*c))
                .collect(),
        ),
        "countOfUnicodes" => Value::Int(g.codepoints.len() as i64),
        "countOfLayers" => Value::Int(layers().count() as i64),
        "countOfPartsSettings" => Value::Int(g.component_axes.len() as i64),
        "direction" => match g.direction {
            None | Some(Direction::LeftToRight) => Value::Int(0),
            Some(Direction::Bidi) => Value::Int(1),
            Some(Direction::RightToLeft) => Value::Int(2),
            Some(_) => return None,
        },
        "locked" => Value::Bool(fs.get_bool("locked")),
        "hasAnnotations" => Value::Bool(layers().any(|l| {
            l.format_specific
                .get(KEY_ANNOTATIONS)
                .and_then(|a| a.as_array())
                .is_some_and(|a| !a.is_empty())
        })),
        "hasComponents" => {
            Value::Bool(layers().any(|l| l.shapes.iter().any(|s| matches!(s, Shape::Component(_)))))
        }
        "hasCorners" | "isCornerGlyph" => Value::Bool(has_hint_type(g, &["Corner"])),
        "hasHints" => Value::Bool(layers().any(|l| !layer_hints(l).is_empty())),
        "hasPostScriptHints" => Value::Bool(has_hint_type(
            g,
            &["Stem", "TopGhost", "BottomGhost", "Flex"],
        )),
        "hasTrueTypeHints" => Value::Bool(has_hint_type(
            g,
            &[
                "TTStem",
                "TTAnchor",
                "TTAlign",
                "TTInterpolate",
                "TTDiagonal",
                "TTDelta",
                "TTSnap",
            ],
        )),
        "hasSpecialLayers" => Value::Bool(has_attr(
            g,
            &[
                "color",
                "colorPalette",
                "sbixSize",
                "svg",
                "coordinates",
                "axisRules",
            ],
        )),
        "isFullColorGlyph" => Value::Bool(has_attr(g, &["color"])),
        "isColorPaletteGlyph" => Value::Bool(has_attr(g, &["colorPalette"])),
        "isAppleColorGlyph" => Value::Bool(has_attr(g, &["sbixSize"])),
        "isSVGColorGlyph" => Value::Bool(has_attr(g, &["svg"])),
        "isAnyColorGlyph" => {
            Value::Bool(has_attr(g, &["color", "colorPalette", "sbixSize", "svg"]))
        }
        "isSmartGlyph" => Value::Bool(g.is_smart_component()),
        _ => return None,
    })
}

#[cfg(all(test, feature = "glyphs"))]
mod tests {
    use super::*;

    fn font() -> Font {
        crate::load("resources/TokenTest.glyphs").unwrap()
    }

    fn expand(font: &Font, predicate: &str) -> Result<String, BabelfontError> {
        Ok(try_resolve_glyphs_predicate_class(font, predicate)?
            .iter()
            .map(|g| g.as_str())
            .collect::<Vec<_>>()
            .join(" "))
    }

    #[test]
    fn test_predicates() {
        let font = font();
        let cases: &[(&str, &str)] = &[
            // Tests from the Glyphs tutorial, as ported from glyphsLib
            ("$[name endswith '.sc']", "A.sc"),
            ("$[not name endswith '.sc']", "A C D E F G H I space Sacute"),
            ("$[NOT name endswith '.sc']", "A C D E F G H I space Sacute"),
            ("$[! name endswith '.sc']", "A C D E F G H I space Sacute"),
            (
                "$[name endswith '.sc' or not name endswith '.sc']",
                "A.sc A C D E F G H I space Sacute",
            ),
            (
                "$[name ENDSWITH '.sc' OR NOT name ENDSWITH '.sc']",
                "A.sc A C D E F G H I space Sacute",
            ),
            (
                "$[name endswith '.sc' || ! name endswith '.sc']",
                "A.sc A C D E F G H I space Sacute",
            ),
            ("$[name endswith '.sc' and not name endswith '.sc']", ""),
            ("$[name ENDSWITH '.sc' AND NOT name ENDSWITH '.sc']", ""),
            ("$[name endswith '.sc' && ! name endswith '.sc']", ""),
            ("$[name like 'A']", "A"),
            ("$[name like 'Sacut']", ""),
            ("$[name like 'S?cute']", "Sacute"),
            ("$[name like 'S*']", "Sacute"),
            ("$[colorIndex == 5]", "Sacute"),
            (r#"$[name matches "S|s.*"]"#, "A.sc space Sacute"),
            (r#"$[script like "latin"]"#, "A C"),
            (r#"$[category like "Separator"]"#, "Sacute"),
            (r#"$[leftKerningGroup like "H"]"#, "A"),
            (r#"$[rightKerningGroup like "L"]"#, "A"),
            (r#"$[unicode beginswith "0041"]"#, "A"),
            (r#"$[note contains "love it"]"#, "A.sc"),
            ("$[countOfUnicodes > 1]", "space"),
            ("$[countOfLayers > 2]", "C D E F G H space"),
            (r#"$[subCategory like "Arrow"]"#, "Sacute"),
            ("$[isAnyColorGlyph == true]", "C D E F"),
            ("$[isFullColorGlyph == true]", "C"),
            ("$[isSVGColorGlyph == true]", "F"),
            ("$[isAppleColorGlyph == true]", "E"),
            (r#"$[script == "latin"]"#, "A C"),
            (r#"$[hasComponents == true and script == "latin"]"#, "C"),
            ("$[hasAnnotations == true]", "I"),
            ("$[hasCorners == true]", "I"),
            ("$[hasSpecialLayers == yes]", "C D E F G H"),
            (r#"$[tags contains "TestTag"]"#, ""),
            (r#"$[tags contains "TestTag1"]"#, "A E"),
            (r#"$[tags contains "TestTag2"]"#, "D E"),
            (r#"$[tags contains "TestTag3"]"#, "A"),
            (r#"$["TestTag1" in tags]"#, "A E"),
            (r#"$["TestTag2" in tags]"#, "D E"),
            (r#"$["TestTag" in tags]"#, ""),
            ("$[countOfLayers between {2, 2}]", "A I A.sc Sacute"),
            ("$[countOfLayers between {3, 3}]", "C D E F G H space"),
            (r#"$[script in {"latin"}]"#, "A C"),
        ];
        for (predicate, expected) in cases {
            assert_eq!(
                expand(&font, predicate).unwrap_or_else(|e| panic!("{predicate}: {e}")),
                *expected,
                "predicate {predicate}"
            );
        }
    }

    #[test]
    fn test_predicate_errors() {
        let font = font();
        for predicate in [
            r#"$[script in {"latin",}]"#,
            r#"$[nme endswith ".sc"]"#,
            "$[name endswith]",
            "$[name frobnicates 'x']",
        ] {
            assert!(expand(&font, predicate).is_err(), "predicate {predicate}");
        }
    }

    #[test]
    fn test_delimiters_optional() {
        let font = font();
        assert_eq!(expand(&font, "name endswith '.sc'").unwrap(), "A.sc");
    }

    #[test]
    fn test_unparseable_predicate_resolves_to_nothing() {
        assert!(resolve_glyphs_predicate_class(&font(), "$[nme endswith '.sc']").is_empty());
    }

    #[test]
    fn test_expand_in_fea() {
        let font = font();
        let expand = |code, output| expand_predicates_in_fea(&font, code, output).unwrap();
        assert_eq!(
            expand("sub a by $[name endswith '.sc'];", PredicateOutput::Class),
            "sub a by [A.sc];"
        );
        assert_eq!(
            expand(" $[name endswith '.sc']\n", PredicateOutput::Bare),
            " A.sc\n"
        );
        // Comments, strings, and other dollar tokens are untouched, and a `]`
        // inside a quoted value doesn't end the predicate
        let untouched = "# $[name like 'A']\nname 1 \"$[x\"; pos a ${padding};";
        assert_eq!(expand(untouched, PredicateOutput::Class), untouched);
        assert_eq!(
            expand(r#"$[name matches "^[AC]$"]"#, PredicateOutput::Class),
            "[A C]"
        );
        assert!(
            expand_predicates_in_fea(&font, "$[name like 'A'", PredicateOutput::Class).is_err()
        );
        assert!(expand_predicates_in_fea(&font, "$[bogus == 1]", PredicateOutput::Class).is_err());
    }

    #[test]
    fn test_fnmatch() {
        assert!(fnmatch("S[a-z]cute", "Sacute"));
        assert!(!fnmatch("S[!a]cute", "Sacute"));
        assert!(fnmatch("a.b", "a.b"));
        assert!(!fnmatch("a.b", "axb"));
    }

    #[test]
    fn test_expand_token_test_font() {
        let mut font = crate::load("resources/TokenTest.glyphs").unwrap();
        ExpandGlyphs3Predicates::new().apply(&mut font).unwrap();
        let sc = &font.features.classes["SmallCaps"].code;
        assert_eq!(sc.trim(), "A.sc");
        assert!(!font.features.to_fea().contains("$["));
    }

    #[test]
    fn test_expand_prefix_and_feature() {
        let mut font = crate::load("resources/TokenTest.glyphs").unwrap();
        font.features.prefixes.insert(
            "P".into(),
            crate::features::PossiblyAutomaticCode::new("@X = $[name like 'A'];"),
        );
        font.features.features.push((
            "liga".into(),
            crate::features::PossiblyAutomaticCode::new("sub $[tags contains 'TestTag2'] by A;"),
        ));
        ExpandGlyphs3Predicates::new().apply(&mut font).unwrap();
        assert_eq!(font.features.prefixes["P"].code, "@X = [A];");
        assert_eq!(
            font.features.features.last().unwrap().1.code,
            "sub [D E] by A;"
        );
    }

    #[test]
    fn test_error_leaves_features_intact() {
        let mut font = crate::load("resources/TokenTest.glyphs").unwrap();
        font.features.features.push((
            "liga".into(),
            crate::features::PossiblyAutomaticCode::new("sub $[bogus == 1] by A;"),
        ));
        let before = font.features.to_fea();
        assert!(ExpandGlyphs3Predicates::new().apply(&mut font).is_err());
        assert_eq!(font.features.to_fea(), before);
    }
}
