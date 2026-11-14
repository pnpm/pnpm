use super::{DocumentPlaceholders, RestoringDeserializer, SeenKeys, visitor::RestoringVisitor};
use serde::{
    Deserializer,
    de::{DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor},
};

struct RestoringSeed<'text, 'keys, Seed> {
    inner: Seed,
    original_text: &'text str,
    placeholders: &'text DocumentPlaceholders,
    seen_keys: Option<&'keys mut SeenKeys>,
}

impl<'de, Seed: DeserializeSeed<'de>> DeserializeSeed<'de> for RestoringSeed<'_, '_, Seed> {
    type Value = Seed::Value;

    fn deserialize<De: Deserializer<'de>>(
        self,
        deserializer: De,
    ) -> Result<Self::Value, De::Error> {
        self.inner.deserialize(RestoringDeserializer {
            inner: deserializer,
            original_text: self.original_text,
            placeholders: self.placeholders,
            seen_keys: self.seen_keys,
        })
    }
}

pub(super) struct RestoringAccess<'text, Inner> {
    pub(super) inner: Inner,
    pub(super) original_text: &'text str,
    pub(super) placeholders: &'text DocumentPlaceholders,
}

impl<'text, Inner> RestoringAccess<'text, Inner> {
    fn wrap_seed<Seed>(&self, seed: Seed) -> RestoringSeed<'text, 'static, Seed> {
        RestoringSeed {
            inner: seed,
            original_text: self.original_text,
            placeholders: self.placeholders,
            seen_keys: None,
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

pub(super) struct RestoringMapAccess<'text, Inner> {
    pub(super) inner: Inner,
    pub(super) original_text: &'text str,
    pub(super) placeholders: &'text DocumentPlaceholders,
    pub(super) seen_keys: SeenKeys,
}

impl<'de, Inner: MapAccess<'de>> MapAccess<'de> for RestoringMapAccess<'_, Inner> {
    type Error = Inner::Error;

    fn next_key_seed<Seed: DeserializeSeed<'de>>(
        &mut self,
        seed: Seed,
    ) -> Result<Option<Seed::Value>, Self::Error> {
        let wrapped = RestoringSeed {
            inner: seed,
            original_text: self.original_text,
            placeholders: self.placeholders,
            seen_keys: Some(&mut self.seen_keys),
        };
        self.inner.next_key_seed(wrapped)
    }

    fn next_value_seed<Seed: DeserializeSeed<'de>>(
        &mut self,
        seed: Seed,
    ) -> Result<Seed::Value, Self::Error> {
        let wrapped = RestoringSeed {
            inner: seed,
            original_text: self.original_text,
            placeholders: self.placeholders,
            seen_keys: None,
        };
        self.inner.next_value_seed(wrapped)
    }

    fn next_entry_seed<KeySeed: DeserializeSeed<'de>, ValueSeed: DeserializeSeed<'de>>(
        &mut self,
        key_seed: KeySeed,
        value_seed: ValueSeed,
    ) -> Result<Option<(KeySeed::Value, ValueSeed::Value)>, Self::Error> {
        let wrapped_key = RestoringSeed {
            inner: key_seed,
            original_text: self.original_text,
            placeholders: self.placeholders,
            seen_keys: Some(&mut self.seen_keys),
        };
        let wrapped_value = RestoringSeed {
            inner: value_seed,
            original_text: self.original_text,
            placeholders: self.placeholders,
            seen_keys: None,
        };
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
                seen_keys: None,
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
                seen_keys: None,
            },
        )
    }
}
