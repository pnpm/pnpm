use super::DocumentPlaceholders;
use serde::{
    Deserialize, Deserializer,
    de::{
        DeserializeOwned, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor,
    },
};
use std::fmt;

/// Deserialize `candidate` YAML into `Settings`, restoring every placeholder
/// sentinel inside string values back to its literal `${...}` slice from
/// `original_text`.
pub(super) fn deserialize_restoring<Settings: DeserializeOwned + Default>(
    candidate: &str,
    original_text: &str,
    placeholders: &DocumentPlaceholders,
) -> Option<Settings> {
    serde_saphyr::with_deserializer_from_str(candidate, |deserializer| {
        Option::<Settings>::deserialize(RestoringDeserializer {
            inner: deserializer,
            original_text,
            placeholders,
        })
    })
    .ok()
    .map(Option::unwrap_or_default)
}

struct RestoringDeserializer<'text, Inner> {
    inner: Inner,
    original_text: &'text str,
    placeholders: &'text DocumentPlaceholders,
}

impl<'text, Inner> RestoringDeserializer<'text, Inner> {
    fn wrap_visitor<Vis>(&self, visitor: Vis) -> RestoringVisitor<'text, Vis> {
        RestoringVisitor {
            inner: visitor,
            original_text: self.original_text,
            placeholders: self.placeholders,
        }
    }
}

macro_rules! forward_deserializer_methods {
    ($($method:ident),+ $(,)?) => {
        $(
            fn $method<Vis: Visitor<'de>>(self, visitor: Vis) -> Result<Vis::Value, Self::Error> {
                let wrapped = self.wrap_visitor(visitor);
                self.inner.$method(wrapped)
            }
        )+
    };
}

impl<'de, Inner: Deserializer<'de>> Deserializer<'de> for RestoringDeserializer<'_, Inner> {
    type Error = Inner::Error;

    forward_deserializer_methods!(
        deserialize_any,
        deserialize_bool,
        deserialize_i8,
        deserialize_i16,
        deserialize_i32,
        deserialize_i64,
        deserialize_i128,
        deserialize_u8,
        deserialize_u16,
        deserialize_u32,
        deserialize_u64,
        deserialize_u128,
        deserialize_f32,
        deserialize_f64,
        deserialize_char,
        deserialize_str,
        deserialize_string,
        deserialize_bytes,
        deserialize_byte_buf,
        deserialize_option,
        deserialize_unit,
        deserialize_seq,
        deserialize_map,
        deserialize_identifier,
        deserialize_ignored_any,
    );

    fn deserialize_unit_struct<Vis: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: Vis,
    ) -> Result<Vis::Value, Self::Error> {
        let wrapped = self.wrap_visitor(visitor);
        self.inner.deserialize_unit_struct(name, wrapped)
    }

    fn deserialize_newtype_struct<Vis: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: Vis,
    ) -> Result<Vis::Value, Self::Error> {
        let wrapped = self.wrap_visitor(visitor);
        self.inner.deserialize_newtype_struct(name, wrapped)
    }

    fn deserialize_tuple<Vis: Visitor<'de>>(
        self,
        len: usize,
        visitor: Vis,
    ) -> Result<Vis::Value, Self::Error> {
        let wrapped = self.wrap_visitor(visitor);
        self.inner.deserialize_tuple(len, wrapped)
    }

    fn deserialize_tuple_struct<Vis: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: Vis,
    ) -> Result<Vis::Value, Self::Error> {
        let wrapped = self.wrap_visitor(visitor);
        self.inner.deserialize_tuple_struct(name, len, wrapped)
    }

    fn deserialize_struct<Vis: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: Vis,
    ) -> Result<Vis::Value, Self::Error> {
        let wrapped = self.wrap_visitor(visitor);
        self.inner.deserialize_struct(name, fields, wrapped)
    }

    fn deserialize_enum<Vis: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: Vis,
    ) -> Result<Vis::Value, Self::Error> {
        let wrapped = self.wrap_visitor(visitor);
        self.inner.deserialize_enum(name, variants, wrapped)
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

struct RestoringVisitor<'text, Inner> {
    inner: Inner,
    original_text: &'text str,
    placeholders: &'text DocumentPlaceholders,
}

macro_rules! forward_visitor_primitives {
    ($($method:ident => $ty:ty),+ $(,)?) => {
        $(
            fn $method<Err: serde::de::Error>(self, value: $ty) -> Result<Self::Value, Err> {
                self.inner.$method(value)
            }
        )+
    };
}

impl<'de, Inner: Visitor<'de>> Visitor<'de> for RestoringVisitor<'_, Inner> {
    type Value = Inner::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.expecting(formatter)
    }

    forward_visitor_primitives!(
        visit_bool => bool,
        visit_i8 => i8,
        visit_i16 => i16,
        visit_i32 => i32,
        visit_i64 => i64,
        visit_i128 => i128,
        visit_u8 => u8,
        visit_u16 => u16,
        visit_u32 => u32,
        visit_u64 => u64,
        visit_u128 => u128,
        visit_f32 => f32,
        visit_f64 => f64,
        visit_char => char,
        visit_bytes => &[u8],
        visit_borrowed_bytes => &'de [u8],
        visit_byte_buf => Vec<u8>,
    );

    fn visit_str<Err: serde::de::Error>(self, value: &str) -> Result<Self::Value, Err> {
        if self.placeholders.may_contain_sentinel(value) {
            self.inner.visit_string(self.placeholders.restore(self.original_text, value))
        } else {
            self.inner.visit_str(value)
        }
    }

    fn visit_borrowed_str<Err: serde::de::Error>(
        self,
        value: &'de str,
    ) -> Result<Self::Value, Err> {
        if self.placeholders.may_contain_sentinel(value) {
            self.inner.visit_string(self.placeholders.restore(self.original_text, value))
        } else {
            self.inner.visit_borrowed_str(value)
        }
    }

    fn visit_string<Err: serde::de::Error>(self, value: String) -> Result<Self::Value, Err> {
        if self.placeholders.may_contain_sentinel(&value) {
            self.inner.visit_string(self.placeholders.restore(self.original_text, &value))
        } else {
            self.inner.visit_string(value)
        }
    }

    fn visit_none<Err: serde::de::Error>(self) -> Result<Self::Value, Err> {
        self.inner.visit_none()
    }

    fn visit_some<De: Deserializer<'de>>(self, deserializer: De) -> Result<Self::Value, De::Error> {
        self.inner.visit_some(RestoringDeserializer {
            inner: deserializer,
            original_text: self.original_text,
            placeholders: self.placeholders,
        })
    }

    fn visit_unit<Err: serde::de::Error>(self) -> Result<Self::Value, Err> {
        self.inner.visit_unit()
    }

    fn visit_newtype_struct<De: Deserializer<'de>>(
        self,
        deserializer: De,
    ) -> Result<Self::Value, De::Error> {
        self.inner.visit_newtype_struct(RestoringDeserializer {
            inner: deserializer,
            original_text: self.original_text,
            placeholders: self.placeholders,
        })
    }

    fn visit_seq<Seq: SeqAccess<'de>>(self, seq: Seq) -> Result<Self::Value, Seq::Error> {
        self.inner.visit_seq(RestoringAccess {
            inner: seq,
            original_text: self.original_text,
            placeholders: self.placeholders,
        })
    }

    fn visit_map<Map: MapAccess<'de>>(self, map: Map) -> Result<Self::Value, Map::Error> {
        self.inner.visit_map(RestoringAccess {
            inner: map,
            original_text: self.original_text,
            placeholders: self.placeholders,
        })
    }

    fn visit_enum<Enum: EnumAccess<'de>>(self, data: Enum) -> Result<Self::Value, Enum::Error> {
        self.inner.visit_enum(RestoringAccess {
            inner: data,
            original_text: self.original_text,
            placeholders: self.placeholders,
        })
    }
}

struct RestoringSeed<'text, Seed> {
    inner: Seed,
    original_text: &'text str,
    placeholders: &'text DocumentPlaceholders,
}

impl<'de, Seed: DeserializeSeed<'de>> DeserializeSeed<'de> for RestoringSeed<'_, Seed> {
    type Value = Seed::Value;

    fn deserialize<De: Deserializer<'de>>(
        self,
        deserializer: De,
    ) -> Result<Self::Value, De::Error> {
        self.inner.deserialize(RestoringDeserializer {
            inner: deserializer,
            original_text: self.original_text,
            placeholders: self.placeholders,
        })
    }
}

struct RestoringAccess<'text, Inner> {
    inner: Inner,
    original_text: &'text str,
    placeholders: &'text DocumentPlaceholders,
}

impl<'text, Inner> RestoringAccess<'text, Inner> {
    fn wrap_seed<Seed>(&self, seed: Seed) -> RestoringSeed<'text, Seed> {
        RestoringSeed {
            inner: seed,
            original_text: self.original_text,
            placeholders: self.placeholders,
        }
    }
}

impl<'de, Inner: SeqAccess<'de>> SeqAccess<'de> for RestoringAccess<'_, Inner> {
    type Error = Inner::Error;

    fn next_element_seed<Seed: DeserializeSeed<'de>>(
        &mut self,
        seed: Seed,
    ) -> Result<Option<Seed::Value>, Self::Error> {
        let wrapped = self.wrap_seed(seed);
        self.inner.next_element_seed(wrapped)
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

impl<'de, Inner: MapAccess<'de>> MapAccess<'de> for RestoringAccess<'_, Inner> {
    type Error = Inner::Error;

    fn next_key_seed<Seed: DeserializeSeed<'de>>(
        &mut self,
        seed: Seed,
    ) -> Result<Option<Seed::Value>, Self::Error> {
        let wrapped = self.wrap_seed(seed);
        self.inner.next_key_seed(wrapped)
    }

    fn next_value_seed<Seed: DeserializeSeed<'de>>(
        &mut self,
        seed: Seed,
    ) -> Result<Seed::Value, Self::Error> {
        let wrapped = self.wrap_seed(seed);
        self.inner.next_value_seed(wrapped)
    }

    fn next_entry_seed<KeySeed: DeserializeSeed<'de>, ValueSeed: DeserializeSeed<'de>>(
        &mut self,
        key_seed: KeySeed,
        value_seed: ValueSeed,
    ) -> Result<Option<(KeySeed::Value, ValueSeed::Value)>, Self::Error> {
        let wrapped_key = self.wrap_seed(key_seed);
        let wrapped_value = self.wrap_seed(value_seed);
        self.inner.next_entry_seed(wrapped_key, wrapped_value)
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

impl<'de, 'text, Inner: EnumAccess<'de>> EnumAccess<'de> for RestoringAccess<'text, Inner> {
    type Error = Inner::Error;
    type Variant = RestoringAccess<'text, Inner::Variant>;

    fn variant_seed<Seed: DeserializeSeed<'de>>(
        self,
        seed: Seed,
    ) -> Result<(Seed::Value, Self::Variant), Self::Error> {
        let original_text = self.original_text;
        let placeholders = self.placeholders;
        let wrapped = self.wrap_seed(seed);
        let (variant, access) = self.inner.variant_seed(wrapped)?;
        Ok((variant, RestoringAccess { inner: access, original_text, placeholders }))
    }
}

impl<'de, Inner: VariantAccess<'de>> VariantAccess<'de> for RestoringAccess<'_, Inner> {
    type Error = Inner::Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        self.inner.unit_variant()
    }

    fn newtype_variant_seed<Seed: DeserializeSeed<'de>>(
        self,
        seed: Seed,
    ) -> Result<Seed::Value, Self::Error> {
        let wrapped = self.wrap_seed(seed);
        self.inner.newtype_variant_seed(wrapped)
    }

    fn tuple_variant<Vis: Visitor<'de>>(
        self,
        len: usize,
        visitor: Vis,
    ) -> Result<Vis::Value, Self::Error> {
        self.inner.tuple_variant(
            len,
            RestoringVisitor {
                inner: visitor,
                original_text: self.original_text,
                placeholders: self.placeholders,
            },
        )
    }

    fn struct_variant<Vis: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: Vis,
    ) -> Result<Vis::Value, Self::Error> {
        self.inner.struct_variant(
            fields,
            RestoringVisitor {
                inner: visitor,
                original_text: self.original_text,
                placeholders: self.placeholders,
            },
        )
    }
}
