//! Decoding YAML in tests through both of the lockfile types' decoders.

use std::fmt::Debug;

/// Decode `yaml` with serde (the decoder `serde_json::from_value` callers
/// use) and with deser (the decoder [`crate::Lockfile::parse`] uses), and
/// require the two to agree: both succeed with equal values, or both fail.
/// A failure carries both messages.
pub(crate) fn from_str<Value>(yaml: &str) -> Result<Value, String>
where
    Value: serde::de::DeserializeOwned + for<'de> deser::Deserialize<'de> + PartialEq + Debug,
{
    match (serde_saphyr::from_str::<Value>(yaml), deser_yaml::from_str::<Value>(yaml)) {
        (Ok(serde), Ok(deser)) => {
            assert_eq!(serde, deser, "serde and deser decode {yaml:?} differently");
            Ok(serde)
        }
        (Err(serde), Err(deser)) => Err(format!("serde: {serde}\ndeser: {deser}")),
        (serde, deser) => {
            panic!("only one decoder accepts {yaml:?}:\nserde: {serde:?}\ndeser: {deser:?}")
        }
    }
}
