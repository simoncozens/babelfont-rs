use std::collections::HashMap;

use fontdrasil::coords::{DesignCoord, UserCoord};
use serde::{
    ser::{SerializeMap as _, SerializeSeq as _},
    Deserialize as _,
};

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

pub(crate) fn usercoord_option_ser<S>(
    value: &Option<UserCoord>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(v) => serializer.serialize_f64(v.to_f64()),
        None => serializer.serialize_none(),
    }
}

pub(crate) fn usercoord_option_de<'de, D>(deserializer: D) -> Result<Option<UserCoord>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<f64> = Option::deserialize(deserializer)?;
    Ok(opt.map(|v| UserCoord::new(v)))
}

pub(crate) fn axismap_ser<S>(
    map: &Option<Vec<(UserCoord, DesignCoord)>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match map {
        Some(pairs) => {
            let mut ser_vec = serializer.serialize_seq(Some(pairs.len()))?;
            for (user, design) in pairs {
                ser_vec.serialize_element(&(user.to_f64(), design.to_f64()))?;
            }
            ser_vec.end()
        }
        None => serializer.serialize_none(),
    }
}

pub(crate) fn axismap_de<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<(UserCoord, DesignCoord)>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<Vec<(f64, f64)>> = Option::deserialize(deserializer)?;
    Ok(opt.map(|pairs| {
        pairs
            .into_iter()
            .map(|(u, d)| (UserCoord::new(u), DesignCoord::new(d)))
            .collect()
    }))
}
