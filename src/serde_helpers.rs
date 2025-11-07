use std::collections::HashMap;

use serde::{ser::SerializeMap as _, Deserialize as _};

pub(crate) fn kerning_map<S>(
    map: &HashMap<(String, String), i16>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let mut ser_map = serializer.serialize_map(Some(map.len()))?;
    for ((left, right), value) in map {
        let key = format!("{}:{}", left, right);
        ser_map.serialize_entry(&key, value)?;
    }
    ser_map.end()
}

pub(crate) fn kerning_unmap<'de, D>(
    deserializer: D,
) -> Result<HashMap<(String, String), i16>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw_map: HashMap<String, i16> = HashMap::deserialize(deserializer)?;
    let mut map = HashMap::new();
    for (key, value) in raw_map {
        let parts: Vec<&str> = key.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(serde::de::Error::custom(format!(
                "Invalid kerning key format: {}",
                key
            )));
        }
        map.insert((parts[0].to_string(), parts[1].to_string()), value);
    }
    Ok(map)
}
