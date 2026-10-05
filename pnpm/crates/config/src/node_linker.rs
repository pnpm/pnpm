use crate::workspace_yaml::deserialize_double_option;
use crate::{Config, HoistingLimits, NodeLinker, WorkspaceSettings};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NodeLinkerSetting {
    Type(NodeLinker),
    Options(NodeLinkerOptions),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum NodeLinkerOptions {
    Isolated {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_double_option"
        )]
        hoist: Option<Option<HoistSetting>>,
    },
    Hoisted {
        #[serde(
            default,
            rename = "hoistingLimits",
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_double_option"
        )]
        hoisting_limits: Option<Option<HoistingLimits>>,
    },
    Loaded {
        #[serde(default)]
        excluded: Vec<String>,
    },
}

impl From<NodeLinker> for NodeLinkerSetting {
    fn from(linker: NodeLinker) -> Self {
        Self::Type(linker)
    }
}

impl NodeLinkerSetting {
    /// Apply the selector and the options present in this source.
    pub fn apply_to(self, config: &mut Config) {
        match self {
            Self::Options(
                NodeLinkerOptions::Isolated { .. } | NodeLinkerOptions::Hoisted { .. },
            ) => {
                WorkspaceSettings { node_linker: Some(self), ..WorkspaceSettings::default() }
                    .apply_to(config, Path::new("."));
            }
            Self::Type(linker) => {
                config.node_linker = linker;
                config.node_linker_excluded.clear();
            }
            Self::Options(NodeLinkerOptions::Loaded { excluded }) => {
                config.node_linker = NodeLinker::Loaded;
                config.node_linker_excluded = excluded;
            }
        }
    }

    pub(crate) fn from_resolved(config: &Config) -> Self {
        if config.node_linker == NodeLinker::Loaded {
            Self::Options(NodeLinkerOptions::Loaded {
                excluded: config.node_linker_excluded.clone(),
            })
        } else if config.node_linker == NodeLinker::Isolated {
            Self::Options(NodeLinkerOptions::Isolated {
                hoist: Some(Some(HoistSetting::Patterns(IsolatedHoistPatterns {
                    public: Some(Some(config.public_hoist_pattern.clone().unwrap_or_default())),
                    private: Some(Some(if config.hoist {
                        config.hoist_pattern.clone().unwrap_or_default()
                    } else {
                        vec![]
                    })),
                }))),
            })
        } else if config.node_linker == NodeLinker::Hoisted {
            Self::Options(NodeLinkerOptions::Hoisted {
                hoisting_limits: Some(Some(config.hoisting_limits)),
            })
        } else {
            Self::Type(config.node_linker)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HoistSetting {
    Enabled(bool),
    Patterns(IsolatedHoistPatterns),
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolatedHoistPatterns {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_double_option"
    )]
    pub public: Option<Option<Vec<String>>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_double_option"
    )]
    pub private: Option<Option<Vec<String>>>,
}

impl WorkspaceSettings {
    pub(crate) fn normalize_linker_settings(&mut self) {
        if let Some(shamefully) = self.shamefully_hoist {
            self.public_hoist_pattern =
                Some(Some(if shamefully { vec!["*".into()] } else { vec![] }));
        }
        let Some(NodeLinkerSetting::Options(options)) = self.node_linker.clone() else { return };
        match options {
            NodeLinkerOptions::Isolated { hoist } => {
                if let Some(hoist) = hoist {
                    self.normalize_hoist(hoist.unwrap_or(HoistSetting::Enabled(true)));
                }
                self.node_linker = Some(NodeLinkerSetting::Type(NodeLinker::Isolated));
            }
            NodeLinkerOptions::Hoisted { hoisting_limits } => {
                if let Some(limits) = hoisting_limits {
                    self.hoisting_limits = Some(limits.unwrap_or_default());
                }
                self.node_linker = Some(NodeLinkerSetting::Type(NodeLinker::Hoisted));
            }
            NodeLinkerOptions::Loaded { .. } => {}
        }
    }

    fn normalize_hoist(&mut self, hoist: HoistSetting) {
        match hoist {
            HoistSetting::Enabled(enabled) => {
                self.hoist = Some(enabled);
                self.hoist_pattern = Some(Some(if enabled { vec!["*".into()] } else { vec![] }));
                self.public_hoist_pattern = Some(Some(vec![]));
            }
            HoistSetting::Patterns(patterns) => {
                if let Some(public) = patterns.public {
                    self.public_hoist_pattern = Some(Some(public.unwrap_or_default()));
                }
                if let Some(private) = patterns.private {
                    self.hoist = Some(true);
                    self.hoist_pattern = Some(Some(private.unwrap_or_else(|| vec!["*".into()])));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
