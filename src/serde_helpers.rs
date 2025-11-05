use std::collections::HashMap;

use serde::ser::SerializeMap as _;

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
