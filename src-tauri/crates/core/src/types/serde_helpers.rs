use serde::{Deserialize, Deserializer};

/// Deserialize `Option<Option<T>>` so that a JSON `null` becomes `Some(None)`
/// while a missing field (via `#[serde(default)]`) stays `None`.
pub(super) fn deserialize_double_option<'de, T, D>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}
