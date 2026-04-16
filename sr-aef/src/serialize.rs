use fea_rs_ast::AsFea;
use fea_rs_ast::{GlyphClass, LookupReferenceStatement};
use indexmap::{IndexMap, IndexSet};
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use skrifa::Tag;
use smol_str::SmolStr;
use std::collections::HashMap;

pub(crate) fn serialize_lookup_map<S>(
    lookup_map: &HashMap<(String, u16), SmolStr>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut map = serializer.serialize_map(Some(lookup_map.len()))?;
    for ((kind, index), name) in lookup_map {
        let key = format!("{}-{}", kind, index);
        map.serialize_entry(&key, name)?;
    }
    map.end()
}

pub(crate) fn serialize_language_systems<S>(
    language_systems: &IndexMap<Tag, IndexSet<Tag>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut map = serializer.serialize_map(Some(language_systems.len()))?;
    for (script_tag, lang_sys_tags) in language_systems {
        let lang_sys_tags_vec: Vec<String> =
            lang_sys_tags.iter().map(|tag| tag.to_string()).collect();
        map.serialize_entry(&script_tag.to_string(), &lang_sys_tags_vec)?;
    }
    map.end()
}

pub(crate) fn serialize_features<S>(
    features: &IndexMap<SmolStr, Vec<LookupReferenceStatement>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut map = serializer.serialize_map(Some(features.len()))?;
    for (feature_name, lookups) in features {
        map.serialize_entry(
            &feature_name.to_string(),
            &lookups
                .iter()
                .map(|l| l.lookup_name.to_string())
                .collect::<Vec<String>>(),
        )?;
    }
    map.end()
}

pub(crate) fn serialize_named_classes<S>(
    named_classes: &IndexMap<SmolStr, GlyphClass>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut map = serializer.serialize_map(Some(named_classes.len()))?;
    for (class_name, glyph_class) in named_classes {
        map.serialize_entry(&class_name.to_string(), &glyph_class.as_fea(""))?;
    }
    map.end()
}
