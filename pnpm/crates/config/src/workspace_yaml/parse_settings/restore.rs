mod access;
mod visitor;

use self::visitor::RestoringVisitor;
use super::DocumentPlaceholders;
use serde::{
    Deserialize, Deserializer,
    de::{DeserializeOwned, Visitor},
};
use std::collections::HashSet;

/// Keys seen so far in one YAML mapping. Collisions are rejected only when at
/// least one side had a sentinel restored, so YAML-distinct raw keys that
/// stringify alike (`"1"` and `1`) behave identically whether placeholders
/// elsewhere are expanded or sentinels.
#[derive(Default)]
pub(super) struct SeenKeys {
    raw: HashSet<String>,
    restored: HashSet<String>,
}

impl SeenKeys {
    pub(super) fn record(&mut self, key: &str, was_restored: bool) -> bool {
        if was_restored {
            !self.raw.contains(key) && self.restored.insert(key.to_owned())
        } else if self.restored.contains(key) {
            false
        } else {
            self.raw.insert(key.to_owned());
            true
        }
    }
}

/// Deserialize `candidate` YAML into `Settings`, restoring every placeholder
/// sentinel inside string values back to its decoded `${...}` slice from
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
            seen_keys: None,
        })
    })
    .ok()
    .map(Option::unwrap_or_default)
}

pub(super) struct RestoringDeserializer<'text, 'keys, Inner> {
    pub(super) inner: Inner,
    pub(super) original_text: &'text str,
    pub(super) placeholders: &'text DocumentPlaceholders,
    pub(super) seen_keys: Option<&'keys mut SeenKeys>,
}

impl<'text, 'keys, Inner> RestoringDeserializer<'text, 'keys, Inner> {
    fn into_parts<Vis>(self, visitor: Vis) -> (Inner, RestoringVisitor<'text, 'keys, Vis>) {
        (
            self.inner,
            RestoringVisitor {
                inner: visitor,
                original_text: self.original_text,
                placeholders: self.placeholders,
                seen_keys: self.seen_keys,
            },
        )
    }
}

macro_rules! forward_deserializer_methods {
    ($($method:ident),+ $(,)?) => {
        $(
            fn $method<Vis: Visitor<'de>>(self, visitor: Vis) -> Result<Vis::Value, Self::Error> {
                let (inner, wrapped) = self.into_parts(visitor);
                inner.$method(wrapped)
            }
        )+
    };
}

impl<'de, Inner: Deserializer<'de>> Deserializer<'de> for RestoringDeserializer<'_, '_, Inner> {
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
        let (inner, wrapped) = self.into_parts(visitor);
        inner.deserialize_unit_struct(name, wrapped)
    }

    fn deserialize_newtype_struct<Vis: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: Vis,
    ) -> Result<Vis::Value, Self::Error> {
        let (inner, wrapped) = self.into_parts(visitor);
        inner.deserialize_newtype_struct(name, wrapped)
    }

    fn deserialize_tuple<Vis: Visitor<'de>>(
        self,
        len: usize,
        visitor: Vis,
    ) -> Result<Vis::Value, Self::Error> {
        let (inner, wrapped) = self.into_parts(visitor);
        inner.deserialize_tuple(len, wrapped)
    }

    fn deserialize_tuple_struct<Vis: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: Vis,
    ) -> Result<Vis::Value, Self::Error> {
        let (inner, wrapped) = self.into_parts(visitor);
        inner.deserialize_tuple_struct(name, len, wrapped)
    }

    fn deserialize_struct<Vis: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: Vis,
    ) -> Result<Vis::Value, Self::Error> {
        let (inner, wrapped) = self.into_parts(visitor);
        inner.deserialize_struct(name, fields, wrapped)
    }

    fn deserialize_enum<Vis: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: Vis,
    ) -> Result<Vis::Value, Self::Error> {
        let (inner, wrapped) = self.into_parts(visitor);
        inner.deserialize_enum(name, variants, wrapped)
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}
