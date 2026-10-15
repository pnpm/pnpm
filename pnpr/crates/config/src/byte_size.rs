use serde::Deserialize;

/// A byte size representation parsed from YAML or string input.
///
/// Accepts integer bytes directly or string values with standard unit
/// suffixes (`B`, `KB`, `KiB`, `MB`, `MiB`, `GB`, `GiB`, `TB`, `TiB`,
/// `PB`, `PiB`), case-insensitively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteSize(pub String);

impl<'de> Deserialize<'de> for ByteSize {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        struct ByteSizeVisitor;
        impl serde::de::Visitor<'_> for ByteSizeVisitor {
            type Value = ByteSize;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a byte size string or number of bytes")
            }

            fn visit_str<DeError>(self, value: &str) -> Result<ByteSize, DeError> {
                Ok(ByteSize(value.to_string()))
            }

            fn visit_i64<DeError>(self, value: i64) -> Result<ByteSize, DeError> {
                Ok(ByteSize(value.to_string()))
            }

            fn visit_u64<DeError>(self, value: u64) -> Result<ByteSize, DeError> {
                Ok(ByteSize(value.to_string()))
            }

            fn visit_f64<DeError>(self, value: f64) -> Result<ByteSize, DeError> {
                Ok(ByteSize(value.to_string()))
            }
        }
        deserializer.deserialize_any(ByteSizeVisitor)
    }
}

/// Parse a byte size string with optional SI or binary suffix into bytes.
///
/// Multipliers use binary multiples (powers of 1024) customary for cache
/// and memory budgets (`KB`/`KiB` = 1024, `MB`/`MiB` = 1024^2, `GB`/`GiB` = 1024^3).
/// Returns `None` for negative, empty, non-finite, or out-of-range values.
#[must_use]
pub fn parse_byte_size(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(bytes) = raw.parse::<u64>() {
        return Some(bytes);
    }
    let num_end = raw
        .bytes()
        .position(|byte| !(byte.is_ascii_digit() || byte == b'.'))
        .unwrap_or(raw.len());
    if num_end == 0 {
        return None;
    }
    let num: f64 = raw[..num_end].parse().ok()?;
    if num < 0.0 || !num.is_finite() {
        return None;
    }
    let unit = raw[num_end..].trim().to_ascii_lowercase();
    let multiplier: f64 = match unit.as_str() {
        "" | "b" | "bytes" => 1.0,
        "k" | "kb" | "kib" => 1024.0,
        "m" | "mb" | "mib" => 1024.0 * 1024.0,
        "g" | "gb" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "t" | "tb" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        "p" | "pb" | "pib" => 1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    let total = num * multiplier;
    if total < 0.0 || total > (u64::MAX as f64) {
        return None;
    }
    Some(total as u64)
}

#[cfg(test)]
mod tests;
