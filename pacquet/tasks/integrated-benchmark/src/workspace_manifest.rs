//! Pacquet's prod manifest type
//! (`pacquet_config::workspace_yaml::WorkspaceSettings`) is shaped for
//! deserializing user input as an additive overlay over `Config` — it
//! is `Deserialize`-only, has no `supportedArchitectures` /
//! `allowBuilds` fields, and its semantics are "apply non-`None` fields
//! onto an existing `Config`". The benchmark needs to *emit* a complete
//! workspace manifest including those benchmark-only fields, so this
//! module defines a small `Serialize + Deserialize` shape with just
//! the keys we read and write.

use crate::fixtures::PNPM_WORKSPACE;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MinimalWorkspaceManifest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_install_peers: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_scripts: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lockfile: Option<bool>,
    /// Mirrors pnpm's
    /// [`enableGlobalVirtualStore`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/index.ts#L392-L394).
    /// Effective default is `false` for non-`--global` installs in
    /// both pnpm v11 and pacquet (the `true` assignment in upstream
    /// lives only inside the `pnpm install --global` branch). The
    /// benchmark fixture still pins this to `false` so a future
    /// default flip surfaces in CI rather than silently changing
    /// the on-disk layout being measured: with GVS on, slot
    /// directories move to `<storeDir>/links/<scope>/<name>/<version>/<hash>`,
    /// a different shape from the project-local baseline. A
    /// separate GVS-on benchmark variant lives behind a caller-
    /// supplied `--fixture-dir` with the flag flipped to `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_global_virtual_store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_architectures: Option<SupportedArchitectures>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub allow_builds: BTreeMap<String, bool>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SupportedArchitectures {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub os: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cpu: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub libc: Vec<String>,
}

impl MinimalWorkspaceManifest {
    /// The default manifest the benchmark uses when no `--fixture-dir`
    /// is provided. Loaded from the static
    /// `tasks/integrated-benchmark/src/fixtures/pnpm-workspace.yaml`
    /// text fixture, parallel to how `package.json` and `pnpm-lock.yaml`
    /// are bundled.
    ///
    /// The fixture pins `supportedArchitectures` to every OS/CPU/libc
    /// pnpm releases for, so pnpm on Linux CI doesn't skip darwin-only
    /// optionals (e.g. `fsevents`) while pacquet installs every snapshot
    /// unconditionally — the asymmetry would tilt the benchmark in
    /// pnpm's favour. It also pins `allowBuilds` to `false` for the
    /// three packages whose postinstalls would otherwise trip pnpm's
    /// `ERR_PNPM_IGNORED_BUILDS` warning under `ignore-scripts=true`.
    pub fn default_for_benchmark() -> Self {
        serde_saphyr::from_str(PNPM_WORKSPACE).expect("parse default pnpm-workspace.yaml fixture")
    }
}
