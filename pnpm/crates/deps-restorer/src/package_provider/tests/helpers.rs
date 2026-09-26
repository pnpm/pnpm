use super::super::{PackageProviderError, PackageProviderInputs, ProviderRequestBundle};
use crate::SkippedSnapshots;
use pnpm_config::Config;
use pnpm_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, PkgName, SnapshotDepRef, SnapshotEntry,
    TarballResolution,
};
use pnpm_patching::ExtendedPatchInfo;
use std::{collections::HashMap, path::PathBuf, sync::OnceLock};

pub(super) const INTEGRITY: &str = "sha512-AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gISIjJCUmJygpKissLS4vMDEyMzQ1Njc4OTo7PD0+Pw==";
pub(super) const ENGINE: &str = "linux;x64;node20";

pub(super) fn static_config() -> &'static Config {
    static CONFIG: OnceLock<&'static Config> = OnceLock::new();
    CONFIG.get_or_init(|| Config::default().leak())
}

pub(super) fn key(dep_path: &str) -> PackageKey {
    dep_path.parse().expect("parse package key")
}

pub(super) fn metadata(resolution: LockfileResolution) -> PackageMetadata {
    PackageMetadata {
        resolution,
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

pub(super) fn tarball_metadata() -> PackageMetadata {
    metadata(LockfileResolution::Tarball(TarballResolution {
        tarball: "https://registry.example/foo/-/foo-1.0.0.tgz".to_string(),
        integrity: Some(INTEGRITY.parse().expect("parse integrity")),
        revision: None,
        git_hosted: None,
        path: None,
    }))
}

pub(super) fn snapshot_with_deps(deps: &[(&str, &str)]) -> SnapshotEntry {
    SnapshotEntry {
        dependencies: (!deps.is_empty()).then(|| {
            deps.iter()
                .map(|(alias, dep_ref)| {
                    (
                        PkgName::parse(*alias).expect("parse alias"),
                        dep_ref.parse::<SnapshotDepRef>().expect("parse dep ref"),
                    )
                })
                .collect()
        }),
        ..SnapshotEntry::default()
    }
}

pub(super) struct Fixture {
    pub(super) snapshots: HashMap<PackageKey, SnapshotEntry>,
    pub(super) packages: HashMap<PackageKey, PackageMetadata>,
    pub(super) skipped: SkippedSnapshots,
    pub(super) patches: Option<HashMap<PackageKey, ExtendedPatchInfo>>,
    pub(super) lockfile_dir: PathBuf,
}

impl Fixture {
    pub(super) fn new() -> Self {
        Fixture {
            snapshots: HashMap::new(),
            packages: HashMap::new(),
            skipped: SkippedSnapshots::new(),
            patches: None,
            lockfile_dir: PathBuf::from("/workspace"),
        }
    }

    pub(super) fn with(
        mut self,
        dep_path: &str,
        snapshot: SnapshotEntry,
        meta: PackageMetadata,
    ) -> Self {
        let full_key = key(dep_path);
        self.packages.insert(full_key.without_peer(), meta);
        self.snapshots.insert(full_key, snapshot);
        self
    }

    pub(super) fn inputs(&self) -> PackageProviderInputs<'_> {
        PackageProviderInputs {
            package_provider: "/provider",
            lockfile_dir: &self.lockfile_dir,
            snapshots: Some(&self.snapshots),
            packages: Some(&self.packages),
            skipped: &self.skipped,
            patches: self.patches.as_ref(),
            engine: Some(ENGINE),
            config: static_config(),
        }
    }

    pub(super) fn build(&self) -> Result<Option<ProviderRequestBundle>, PackageProviderError> {
        super::super::build_provider_request(&self.inputs())
    }

    pub(super) fn build_json(&self) -> serde_json::Value {
        let bundle = self
            .build()
            .expect("build request")
            .expect("non-empty request");
        serde_json::to_value(&bundle.request).expect("serialize request")
    }
}
