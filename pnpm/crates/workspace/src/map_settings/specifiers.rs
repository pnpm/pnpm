//! Read specifiers as strings directly from the YAML reader. Buffering the
//! outer map for an untagged enum would turn an unquoted version into a number.

use super::{
    CatalogSettings,
    DefaultCatalog,
    RuleSettings,
    Rules,
};
use indexmap::IndexMap;
use serde::{
    Deserialize,
    Deserializer,
    de::{
        Error,
        MapAccess,
        Visitor,
        value::{
            MapAccessDeserializer,
            StrDeserializer,
        },
    },
};
use std::{
    fmt,
    marker::PhantomData,
};

type Specifiers = IndexMap<String, String>;

trait Section: Sized {
    const KEY: &'static str;
    fn section<Err: Error>(map: Specifiers, options: &mut Specifiers) -> Result<Self, Err>;
    fn flat(map: Specifiers) -> Self;
}

fn policy<'de, Policy: Deserialize<'de>, Err: Error>(
    value: Option<&'de str>,
) -> Result<Option<Policy>, Err> {
    value
        .map(StrDeserializer::new)
        .map(Policy::deserialize)
        .transpose()
}

impl Section for Rules<Specifiers> {
    const KEY: &'static str = "rules";
    fn section<Err: Error>(rules: Specifiers, options: &mut Specifiers) -> Result<Self, Err> {
        Ok(Self::Section(RuleSettings {
            rules,
            unused: policy(options.shift_remove("unused").as_deref())?,
        }))
    }
    fn flat(map: Specifiers) -> Self {
        Self::Flat(map)
    }
}

impl Section for DefaultCatalog<Specifiers> {
    const KEY: &'static str = "entries";
    fn section<Err: Error>(entries: Specifiers, options: &mut Specifiers) -> Result<Self, Err> {
        Ok(Self::Section(CatalogSettings {
            entries,
            mode: policy(options.shift_remove("mode").as_deref())?,
            save_to: options.shift_remove("saveTo"),
            unused: policy(options.shift_remove("unused").as_deref())?,
        }))
    }
    fn flat(map: Specifiers) -> Self {
        Self::Flat(map)
    }
}

struct ReadSection<Setting>(PhantomData<Setting>);

impl<'de, Setting: Section> Visitor<'de> for ReadSection<Setting> {
    type Value = Setting;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a map of package specifiers")
    }
    fn visit_map<Access: MapAccess<'de>>(
        self,
        mut source: Access,
    ) -> Result<Setting, Access::Error> {
        let mut flat = Specifiers::new();
        let mut nested = None;
        while let Some(key) = source.next_key::<String>()? {
            if key == Setting::KEY {
                match source.next_value::<SpecifierOrMap>()? {
                    SpecifierOrMap::Map(entries) => nested = Some(entries),
                    SpecifierOrMap::Specifier(spec) => {
                        flat.insert(key, spec);
                    }
                }
            } else {
                flat.insert(key, source.next_value()?);
            }
        }
        let Some(entries) = nested else {
            return Ok(Setting::flat(flat));
        };
        let section = Setting::section(entries, &mut flat)?;
        if !flat.is_empty() {
            return Err(Access::Error::custom("put all package specifiers inside the nested map"));
        }
        Ok(section)
    }
}

enum SpecifierOrMap {
    Specifier(String),
    Map(Specifiers),
}

impl<'de> Deserialize<'de> for SpecifierOrMap {
    fn deserialize<Deser: Deserializer<'de>>(deserializer: Deser) -> Result<Self, Deser::Error> {
        deserializer.deserialize_any(ReadSpecifier)
    }
}

struct ReadSpecifier;

impl<'de> Visitor<'de> for ReadSpecifier {
    type Value = SpecifierOrMap;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a package specifier or a mapping of specifiers")
    }
    fn visit_map<Access: MapAccess<'de>>(self, map: Access) -> Result<Self::Value, Access::Error> {
        Specifiers::deserialize(MapAccessDeserializer::new(map)).map(SpecifierOrMap::Map)
    }
    fn visit_str<Err: Error>(self, spec: &str) -> Result<Self::Value, Err> {
        Ok(SpecifierOrMap::Specifier(spec.to_owned()))
    }
    fn visit_i64<Err: Error>(self, value: i64) -> Result<Self::Value, Err> {
        Ok(SpecifierOrMap::Specifier(value.to_string()))
    }
    fn visit_u64<Err: Error>(self, value: u64) -> Result<Self::Value, Err> {
        Ok(SpecifierOrMap::Specifier(value.to_string()))
    }
    fn visit_f64<Err: Error>(self, value: f64) -> Result<Self::Value, Err> {
        Ok(SpecifierOrMap::Specifier(serde_json::Value::from(value).to_string()))
    }
    fn visit_bool<Err: Error>(self, value: bool) -> Result<Self::Value, Err> {
        Ok(SpecifierOrMap::Specifier(value.to_string()))
    }
}

struct Parsed<Setting>(Setting);
impl<'de, Setting: Section> Deserialize<'de> for Parsed<Setting> {
    fn deserialize<Deser: Deserializer<'de>>(deserializer: Deser) -> Result<Self, Deser::Error> {
        deserializer
            .deserialize_map(ReadSection(PhantomData))
            .map(Self)
    }
}

pub fn catalog<'de, Deser: Deserializer<'de>>(
    deserializer: Deser,
) -> Result<Option<DefaultCatalog<Specifiers>>, Deser::Error> {
    Ok(Option::<Parsed<DefaultCatalog<Specifiers>>>::deserialize(deserializer)?
        .map(|parsed| parsed.0))
}

pub fn overrides<'de, Deser: Deserializer<'de>>(
    deserializer: Deser,
) -> Result<Option<Rules<Specifiers>>, Deser::Error> {
    Ok(Option::<Parsed<Rules<Specifiers>>>::deserialize(deserializer)?.map(|parsed| parsed.0))
}
