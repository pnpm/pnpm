use crate::{
    Config,
    HoistingLimits,
    NodeLinker,
};
use serde::{
    Deserialize,
    Serialize,
    de::{
        MapAccess,
        Visitor,
        value::MapAccessDeserializer,
    },
};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum NodeLinkerSetting {
    Type(NodeLinker),
    Options(NodeLinkerOptions),
}

impl<'de> Deserialize<'de> for NodeLinkerSetting {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(NodeLinkerSettingVisitor)
    }
}

struct NodeLinkerSettingVisitor;

impl<'de> Visitor<'de> for NodeLinkerSettingVisitor {
    type Value = NodeLinkerSetting;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a linker name or an object with linker options")
    }

    fn visit_str<Error>(self, value: &str) -> Result<Self::Value, Error>
    where
        Error: serde::de::Error,
    {
        let linker = match value {
            "isolated" => NodeLinker::Isolated,
            "hoisted" => NodeLinker::Hoisted,
            "pnp" => NodeLinker::Pnp,
            "loaded" => NodeLinker::Loaded,
            _ => {
                return Err(Error::unknown_variant(
                    value,
                    &["isolated", "hoisted", "pnp", "loaded"],
                ));
            }
        };
        Ok(NodeLinkerSetting::Type(linker))
    }

    fn visit_string<Error>(self, value: String) -> Result<Self::Value, Error>
    where
        Error: serde::de::Error,
    {
        self.visit_str(&value)
    }

    fn visit_map<Map>(self, map: Map) -> Result<Self::Value, Map::Error>
    where
        Map: MapAccess<'de>,
    {
        NodeLinkerOptions::deserialize(MapAccessDeserializer::new(map))
            .map(NodeLinkerSetting::Options)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum NodeLinkerOptions {
    Isolated {
        #[serde(default)]
        hoist: Option<HoistSetting>,
    },
    Hoisted {
        #[serde(default)]
        #[serde(rename = "hoistingLimits")]
        hoisting_limits: Option<HoistingLimits>,
    },
    Pnp {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        _marker: Option<()>,
    },
    Loaded {
        #[serde(default)]
        excluded: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HoistSetting {
    Enabled(bool),
    Patterns(NodeLinkerHoistPatterns),
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeLinkerHoistPatterns {
    pub public: Option<Vec<String>>,
    pub private: Option<Vec<String>>,
}

impl From<NodeLinker> for NodeLinkerSetting {
    fn from(linker: NodeLinker) -> Self {
        Self::Type(linker)
    }
}

impl NodeLinkerSetting {
    /// Apply the selected linker and its options.
    pub fn apply_to(self, config: &mut Config) {
        match self {
            Self::Type(linker) => select_linker(config, linker),
            Self::Options(NodeLinkerOptions::Isolated { hoist }) => {
                select_linker(config, NodeLinker::Isolated);
                apply_isolated_hoist(config, hoist);
            }
            Self::Options(NodeLinkerOptions::Hoisted { hoisting_limits }) => {
                select_linker(config, NodeLinker::Hoisted);
                apply_hoisting_limits(config, hoisting_limits);
            }
            Self::Options(NodeLinkerOptions::Pnp { .. }) => select_linker(config, NodeLinker::Pnp),
            Self::Options(NodeLinkerOptions::Loaded { excluded }) => {
                select_linker(config, NodeLinker::Loaded);
                config.node_linker_excluded = excluded;
            }
        }
    }

    pub(crate) fn from_resolved(config: &Config) -> Self {
        match config.node_linker {
            NodeLinker::Isolated => Self::Options(NodeLinkerOptions::Isolated {
                hoist: Some(if config.hoist {
                    HoistSetting::Patterns(NodeLinkerHoistPatterns {
                        public: config.public_hoist_pattern.clone(),
                        private: config.hoist_pattern.clone(),
                    })
                } else {
                    HoistSetting::Enabled(false)
                }),
            }),
            NodeLinker::Hoisted => Self::Options(NodeLinkerOptions::Hoisted {
                hoisting_limits: Some(config.hoisting_limits),
            }),
            NodeLinker::Loaded => Self::Options(NodeLinkerOptions::Loaded {
                excluded: config.node_linker_excluded.clone(),
            }),
            NodeLinker::Pnp => Self::Type(NodeLinker::Pnp),
        }
    }
}

fn select_linker(config: &mut Config, linker: NodeLinker) {
    config.node_linker = linker;
    config.node_linker_excluded.clear();
}

fn apply_isolated_hoist(config: &mut Config, hoist: Option<HoistSetting>) {
    match hoist {
        Some(HoistSetting::Enabled(false)) => {
            config.hoist = false;
            config.hoist_pattern = None;
            config.public_hoist_pattern = None;
        }
        Some(HoistSetting::Enabled(true)) => {
            config.hoist = true;
            config.hoist_pattern = Some(crate::default_hoist_pattern());
            config.public_hoist_pattern = Some(crate::default_public_hoist_pattern());
        }
        Some(HoistSetting::Patterns(patterns)) => {
            config.hoist = true;
            if let Some(private) = patterns.private {
                config.hoist_pattern = Some(private);
            }
            if let Some(public) = patterns.public {
                config.public_hoist_pattern = Some(public);
            }
        }
        None => {}
    }
}

fn apply_hoisting_limits(config: &mut Config, hoisting_limits: Option<HoistingLimits>) {
    if let Some(hoisting_limits) = hoisting_limits {
        config.hoisting_limits = hoisting_limits;
    }
}

#[cfg(test)]
mod tests;
