use serde::{Deserialize, Serialize};

/// Supply-chain trust policy applied to lockfile entries.
///
/// The setting is `'no-downgrade' | 'off'` and drives the
/// `pnpm-resolving-npm-resolver` verifier: under
/// [`TrustPolicy::NoDowngrade`] the verifier rejects any version
/// whose trust evidence (`_npmUser.trustedPublisher` or
/// `dist.attestations.provenance`) is weaker than an earlier-published
/// version's.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrustPolicy {
    #[default]
    Off,
    NoDowngrade,
}

/// `linkWorkspacePackages` from `pnpm-workspace.yaml`. Tri-state: a
/// bare-semver dependency on a workspace package may resolve to the
/// local copy, or to a registry copy with the same name, or be
/// matched only when the user explicitly opts in with a `workspace:`
/// prefix.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LinkWorkspacePackages {
    /// `false`. Workspace packages are matched only when the user
    /// writes a `workspace:`-prefixed range. A bare-semver range
    /// always goes to the registry.
    #[default]
    Off,
    /// `true`. Direct dependencies match workspace packages by name
    /// and version, like a `workspace:` range would; transitive
    /// dependencies still go to the registry.
    DirectOnly,
    /// `"deep"`. Both direct and transitive dependencies match
    /// workspace packages.
    Deep,
}

impl LinkWorkspacePackages {
    /// Whether the npm resolver should consult the workspace map
    /// when resolving a bare-semver wanted dependency at the given depth.
    #[must_use]
    pub fn enabled_at_depth(self, current_depth: u32) -> bool {
        match self {
            LinkWorkspacePackages::Off => false,
            LinkWorkspacePackages::DirectOnly => current_depth == 0,
            LinkWorkspacePackages::Deep => true,
        }
    }
}

impl serde::Serialize for LinkWorkspacePackages {
    fn serialize<Ser: serde::Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        match self {
            LinkWorkspacePackages::Off => serializer.serialize_bool(false),
            LinkWorkspacePackages::DirectOnly => serializer.serialize_bool(true),
            LinkWorkspacePackages::Deep => serializer.serialize_str("deep"),
        }
    }
}

impl<'de> serde::Deserialize<'de> for LinkWorkspacePackages {
    fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
    where
        De: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::fmt;

        struct V;
        impl Visitor<'_> for V {
            type Value = LinkWorkspacePackages;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(r#"a boolean or the string "deep""#)
            }
            fn visit_bool<DeError: de::Error>(self, value: bool) -> Result<Self::Value, DeError> {
                Ok(if value {
                    LinkWorkspacePackages::DirectOnly
                } else {
                    LinkWorkspacePackages::Off
                })
            }
            fn visit_str<DeError: de::Error>(self, value: &str) -> Result<Self::Value, DeError> {
                match value {
                    "deep" => Ok(LinkWorkspacePackages::Deep),
                    other => Err(DeError::invalid_value(
                        de::Unexpected::Str(other),
                        &r#"true, false, or "deep""#,
                    )),
                }
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// `saveWorkspaceProtocol`. How a dependency linked to a workspace
/// package is written back to `package.json`.
///
/// [`SaveWorkspaceProtocol::Off`] only suppresses the `workspace:`
/// prefix for a dependency that did not already declare one; a
/// `workspace:` specifier always keeps its protocol, so the two
/// non-rolling states behave alike wherever the protocol is already
/// present.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SaveWorkspaceProtocol {
    /// `false`.
    Off,
    /// `true`. The resolved version is written under the protocol,
    /// keeping the range operator the dependency already declared
    /// (`workspace:^1.2.3`).
    On,
    /// `"rolling"`. The range operator is written without a version
    /// (`workspace:*`, `workspace:^`, `workspace:~`), so the entry
    /// never needs rewriting when the workspace package's version
    /// changes.
    #[default]
    Rolling,
}

impl serde::Serialize for SaveWorkspaceProtocol {
    fn serialize<Ser: serde::Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        match self {
            SaveWorkspaceProtocol::Off => serializer.serialize_bool(false),
            SaveWorkspaceProtocol::On => serializer.serialize_bool(true),
            SaveWorkspaceProtocol::Rolling => serializer.serialize_str("rolling"),
        }
    }
}

impl<'de> serde::Deserialize<'de> for SaveWorkspaceProtocol {
    fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
    where
        De: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::fmt;

        struct V;
        impl Visitor<'_> for V {
            type Value = SaveWorkspaceProtocol;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(r#"a boolean or the string "rolling""#)
            }
            fn visit_bool<DeError: de::Error>(self, value: bool) -> Result<Self::Value, DeError> {
                Ok(if value { SaveWorkspaceProtocol::On } else { SaveWorkspaceProtocol::Off })
            }
            fn visit_str<DeError: de::Error>(self, value: &str) -> Result<Self::Value, DeError> {
                match value {
                    "rolling" => Ok(SaveWorkspaceProtocol::Rolling),
                    other => Err(DeError::invalid_value(
                        de::Unexpected::Str(other),
                        &r#"true, false, or "rolling""#,
                    )),
                }
            }
        }
        deserializer.deserialize_any(V)
    }
}
