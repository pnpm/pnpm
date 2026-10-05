use crate::{Config, NodeLinker};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NodeLinkerSetting {
    Type(NodeLinker),
    Options(NodeLinkerOptions),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum NodeLinkerOptions {
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
    /// Replace the selected linker and its exclusions together.
    pub fn apply_to(self, config: &mut Config) {
        let (linker, excluded) = match self {
            Self::Type(linker) => (linker, Vec::new()),
            Self::Options(NodeLinkerOptions::Loaded { excluded }) => (NodeLinker::Loaded, excluded),
        };
        config.node_linker = linker;
        config.node_linker_excluded = excluded;
    }

    pub(crate) fn from_resolved(config: &Config) -> Self {
        if config.node_linker == NodeLinker::Loaded {
            Self::Options(NodeLinkerOptions::Loaded {
                excluded: config.node_linker_excluded.clone(),
            })
        } else {
            Self::Type(config.node_linker)
        }
    }
}

#[cfg(test)]
mod tests;
