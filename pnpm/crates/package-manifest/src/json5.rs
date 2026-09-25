pub(super) use comments::restore_comments;

use serde::{
    Deserialize, Deserializer,
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value};
use std::fmt;

mod comments;

// json5 does not bound recursion. Build the value through a seed so the limit
// applies before descending, including to fields pnpm does not interpret.
const MAX_DEPTH: usize = 128;

pub(super) fn parse(text: &str) -> Result<Value, json5::Error> {
    json5::from_str::<BoundedValue>(text).map(|value| value.0)
}

struct BoundedValue(Value);

impl<'de> Deserialize<'de> for BoundedValue {
    fn deserialize<Decoder: Deserializer<'de>>(
        deserializer: Decoder,
    ) -> Result<Self, Decoder::Error> {
        ValueSeed(0).deserialize(deserializer).map(Self)
    }
}

#[derive(Clone, Copy)]
struct ValueSeed(usize);

impl<'de> DeserializeSeed<'de> for ValueSeed {
    type Value = Value;

    fn deserialize<Decoder: Deserializer<'de>>(
        self,
        deserializer: Decoder,
    ) -> Result<Value, Decoder::Error> {
        if self.0 > MAX_DEPTH {
            return Err(de::Error::custom("manifest nesting exceeds 128 levels"));
        }
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ValueSeed {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON5 value")
    }

    fn visit_unit<Error: de::Error>(self) -> Result<Value, Error> {
        Ok(Value::Null)
    }

    fn visit_bool<Error: de::Error>(self, value: bool) -> Result<Value, Error> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<Error: de::Error>(self, value: i64) -> Result<Value, Error> {
        Ok(value.into())
    }

    fn visit_u64<Error: de::Error>(self, value: u64) -> Result<Value, Error> {
        Ok(value.into())
    }

    fn visit_f64<Error: de::Error>(self, value: f64) -> Result<Value, Error> {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| de::Error::custom("manifest numbers must be finite"))
    }

    fn visit_str<Error: de::Error>(self, value: &str) -> Result<Value, Error> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<Error: de::Error>(self, value: String) -> Result<Value, Error> {
        Ok(Value::String(value))
    }

    fn visit_seq<Access: SeqAccess<'de>>(
        self,
        mut sequence: Access,
    ) -> Result<Value, Access::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(ValueSeed(self.0 + 1))? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<Access: MapAccess<'de>>(
        self,
        mut entries: Access,
    ) -> Result<Value, Access::Error> {
        let mut values = Map::new();
        while let Some(key) = entries.next_key::<String>()? {
            values.insert(key, entries.next_value_seed(ValueSeed(self.0 + 1))?);
        }
        Ok(Value::Object(values))
    }
}

#[cfg(test)]
mod tests;
