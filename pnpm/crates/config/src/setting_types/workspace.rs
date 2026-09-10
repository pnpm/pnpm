use super::{Deserialize, Serialize};

/// `linkWorkspacePackages` from `pnpm-workspace.yaml`. Tri-state: a
/// bare-semver dependency on a workspace package may resolve to the
/// local copy, or to a registry copy with the same name, or be
/// matched only when the user explicitly opts in with a `workspace:`
/// prefix.
///
/// The setting is `linkWorkspacePackages: boolean | 'deep'`. Default is
/// [`LinkWorkspacePackages::Off`] (`'link-workspace-packages': false`).
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
/// The setting is `saveWorkspaceProtocol: boolean | 'rolling'`. Default
/// is [`SaveWorkspaceProtocol::Rolling`]
/// (`'save-workspace-protocol': 'rolling'`).
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

/// How the resolver picks a version for a direct dependency when more
/// than one satisfies the wanted range.
///
/// The setting is `'highest' | 'time-based' | 'lowest-direct'`. Defaults to
/// [`ResolutionMode::Highest`] (`'resolution-mode': 'highest'`).
///
/// Only direct dependencies are affected by the lowest-version pick;
/// subdependencies are always picked highest. Under
/// [`ResolutionMode::TimeBased`] the resolver additionally constrains
/// subdependencies to versions published no later than the newest
/// resolved direct dependency (plus a one-hour delta).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionMode {
    /// Pick the highest version that satisfies the range, everywhere.
    #[default]
    Highest,

    /// Resolve direct dependencies to their lowest satisfying version,
    /// then resolve subdependencies from versions published before the
    /// last direct dependency was published.
    TimeBased,

    /// Resolve direct dependencies to their lowest satisfying version;
    /// subdependencies are unconstrained (picked highest).
    LowestDirect,
}

impl ResolutionMode {
    /// Whether direct dependencies are resolved to their lowest
    /// satisfying version. True for both [`Self::TimeBased`] and
    /// [`Self::LowestDirect`].
    #[must_use]
    pub fn picks_lowest_direct(self) -> bool {
        matches!(self, ResolutionMode::TimeBased | ResolutionMode::LowestDirect)
    }
}

/// How `pnpm add` / `pnpm update` reconcile a directly-specified version
/// against a `catalog:` entry for the same package.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogMode {
    /// The catalog is consulted only for explicit `catalog:` specifiers;
    /// `add` / `update` never reconcile a direct version against it. The
    /// default (`'catalog-mode': 'manual'`).
    #[default]
    Manual,

    /// A direct version that disagrees with the matching catalog entry is
    /// an error (`ERR_PNPM_CATALOG_VERSION_MISMATCH`).
    Strict,

    /// A direct version that disagrees with the matching catalog entry is
    /// kept, with a warning; a version that agrees is used from the
    /// catalog.
    Prefer,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageImportMethod {
    /// Try the platform's cheap link tiers in order — hardlink first on
    /// Linux, clone first elsewhere — and fall back to copying when none
    /// is possible. `deps-restorer::link_file::next_auto_tier` implements
    /// the ladder and carries the rationale.
    #[default]
    Auto,

    /// hard link packages from the store
    Hardlink,

    /// copy packages from the store
    Copy,

    /// clone (AKA copy-on-write or reference link) packages from the store
    Clone,

    /// try to clone packages from the store. If cloning is not supported then fall back to copying
    CloneOrCopy,
}
