//! The peer variants of one package: a map when there is one, a list when
//! there are several.

use super::NextSnapshotEntry;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use std::fmt;

/// The snapshots under one package key. Written as the one snapshot itself
/// when there is one, because nearly every package has one variant and the
/// list would cost a node and two spaces on every line. Read in either shape.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct NextVariants(pub Vec<NextSnapshotEntry>);

impl From<Vec<NextSnapshotEntry>> for NextVariants {
    fn from(variants: Vec<NextSnapshotEntry>) -> Self {
        NextVariants(variants)
    }
}

impl std::ops::Deref for NextVariants {
    type Target = Vec<NextSnapshotEntry>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for NextVariants {
    fn serialize<Ser: Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        match self.0.as_slice() {
            [only] => only.serialize(serializer),
            many => many.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for NextVariants {
    fn deserialize<De: Deserializer<'de>>(deserializer: De) -> Result<Self, De::Error> {
        deserializer.deserialize_any(VariantsVisitor)
    }
}

struct VariantsVisitor;

impl<'de> Visitor<'de> for VariantsVisitor {
    type Value = NextVariants;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a snapshot or a list of snapshots")
    }

    fn visit_map<Access: MapAccess<'de>>(self, map: Access) -> Result<Self::Value, Access::Error> {
        let only = NextSnapshotEntry::deserialize(de::value::MapAccessDeserializer::new(map))?;
        Ok(NextVariants(vec![only]))
    }

    fn visit_seq<Access: SeqAccess<'de>>(
        self,
        mut seq: Access,
    ) -> Result<Self::Value, Access::Error> {
        let mut variants = Vec::with_capacity(seq.size_hint().unwrap_or(2));
        while let Some(variant) = seq.next_element()? {
            variants.push(variant);
        }
        Ok(NextVariants(variants))
    }
}
