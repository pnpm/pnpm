use super::{
    DocumentPlaceholders, RestoringDeserializer, SeenKeys,
    access::{RestoringAccess, RestoringMapAccess},
};
use serde::{
    Deserializer,
    de::{EnumAccess, MapAccess, SeqAccess, Visitor},
};
use std::fmt;

pub(super) struct RestoringVisitor<'text, 'keys, Inner> {
    pub(super) inner: Inner,
    pub(super) original_text: &'text str,
    pub(super) placeholders: &'text DocumentPlaceholders,
    pub(super) seen_keys: Option<&'keys mut SeenKeys>,
}

impl<Inner> RestoringVisitor<'_, '_, Inner> {
    fn record_key<Err: serde::de::Error>(
        &mut self,
        key: &str,
        was_restored: bool,
    ) -> Result<(), Err> {
        if let Some(seen_keys) = self.seen_keys.as_deref_mut()
            && !seen_keys.record(key, was_restored)
        {
            return Err(Err::custom(format!("duplicate mapping key: {key}")));
        }
        Ok(())
    }

    fn restore_str<Err: serde::de::Error>(&mut self, value: &str) -> Result<Option<String>, Err> {
        if !self.placeholders.may_contain_sentinel(value) {
            self.record_key(value, false)?;
            return Ok(None);
        }
        let restored = self.placeholders
            .try_restore(self.original_text, value)
            .ok_or_else(|| Err::custom("invalid escape or quote in placeholder"))?;
        self.record_key(&restored, true)?;
        Ok(Some(restored))
    }
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

impl<'de, Inner: Visitor<'de>> Visitor<'de> for RestoringVisitor<'_, '_, Inner> {
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

    fn visit_str<Err: serde::de::Error>(mut self, value: &str) -> Result<Self::Value, Err> {
        match self.restore_str(value)? {
            Some(restored) => self.inner.visit_string(restored),
            None => self.inner.visit_str(value),
        }
    }

    fn visit_borrowed_str<Err: serde::de::Error>(
        mut self,
        value: &'de str,
    ) -> Result<Self::Value, Err> {
        match self.restore_str(value)? {
            Some(restored) => self.inner.visit_string(restored),
            None => self.inner.visit_borrowed_str(value),
        }
    }

    fn visit_string<Err: serde::de::Error>(mut self, value: String) -> Result<Self::Value, Err> {
        let value = self.restore_str(&value)?.unwrap_or(value);
        self.inner.visit_string(value)
    }

    fn visit_none<Err: serde::de::Error>(self) -> Result<Self::Value, Err> {
        self.inner.visit_none()
    }

    fn visit_some<De: Deserializer<'de>>(self, deserializer: De) -> Result<Self::Value, De::Error> {
        self.inner.visit_some(RestoringDeserializer {
            inner: deserializer,
            original_text: self.original_text,
            placeholders: self.placeholders,
            seen_keys: self.seen_keys,
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
            seen_keys: self.seen_keys,
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
        self.inner.visit_map(RestoringMapAccess {
            inner: map,
            original_text: self.original_text,
            placeholders: self.placeholders,
            seen_keys: SeenKeys::default(),
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
