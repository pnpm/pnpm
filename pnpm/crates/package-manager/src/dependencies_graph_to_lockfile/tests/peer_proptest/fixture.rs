use std::{fs, path::Path};

use pnpm_catalogs_types::Catalogs;
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_deps_resolver::{ResolveImporterOptions, WorkspaceResolveOptions};
use serde_json::{Value, json};

use super::{Reuse, Settings, Workspace, model};

pub(super) struct Fixture {
    pub(super) root: tempfile::TempDir,
    pub(super) manifests: Vec<(String, PackageManifest)>,
    pub(super) catalogs: Catalogs,
}

impl Fixture {
    pub(super) fn frozen_check(
        &self,
        lockfile: &pnpm_lockfile::Lockfile,
        settings: Settings,
    ) -> Result<(), String> {
        let mut config = pnpm_config::Config::new();
        config.workspace_dir = Some(self.root.path().to_path_buf());
        config.lockfile_dir = Some(self.root.path().to_path_buf());
        config.ignore_pnpmfile = true;
        config.lockfile_format = settings.format;
        config.auto_install_peers = settings.auto_install_peers;
        config.peers_suffix_max_length = settings.peers_suffix_max_length.try_into().unwrap();
        config.dedupe_peers = false;
        config.resolve_peers_from_workspace_root = false;
        let check = crate::WantedLockfileSatisfactionCheck {
            config: &config,
            manifest: &self.manifests[0].1,
            catalogs: &self.catalogs,
            lockfile,
            ignore_manifest_check: false,
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(crate::install::tests::check_frozen_fixture(&check, &self.manifests))
    }

    pub(super) fn new(workspace: &Workspace, settings: Settings) -> Self {
        let root = tempfile::tempdir().expect("create workspace");
        let mut catalogs = Catalogs::new();
        let mut manifests = Vec::new();
        for (id, edges) in model::IMPORTER_IDS.iter().zip(&workspace.importers) {
            let mut manifest = json!({
                "name": format!("importer-{}", manifests.len()),
                "version": "1.0.0",
                "dependencies": model::importer_dependencies(edges),
            });
            if settings.catalogs {
                catalogize(&mut manifest, &mut catalogs);
            }
            if settings.injected {
                manifest["dependencies"]["shared"] = json!("workspace:*");
                manifest["dependenciesMeta"] = json!({ "shared": { "injected": true } });
            }
            manifests.push(((*id).to_string(), write_manifest(root.path(), id, &manifest)));
        }
        if settings.injected {
            let mut manifest = model::package_manifest(0, 1, &workspace.packages[0][0]);
            manifest["name"] = json!("shared");
            if settings.catalogs {
                catalogize(&mut manifest, &mut catalogs);
            }
            manifests.push((
                "shared".to_string(),
                write_manifest(root.path(), "shared", &manifest),
            ));
        }
        let yaml = serde_saphyr::to_string(&json!({
            "packages": ["packages/*", "shared"],
            "catalogs": catalogs,
        }))
        .unwrap();
        fs::write(root.path().join("pnpm-workspace.yaml"), yaml).unwrap();
        Self { root, manifests, catalogs }
    }

    pub(super) fn importer_opts(&self, id: &str, settings: Settings) -> ResolveImporterOptions {
        let mut opts = super::importer_opts(id, settings);
        opts.base_opts.project.project_dir = self.root.path().join(id);
        opts.resolution.catalogs = self.catalogs.clone();
        opts.resolution.catalogs_dir = Some(self.root.path().to_path_buf());
        opts.links.lockfile_dir = Some(self.root.path().to_path_buf());
        opts
    }

    pub(super) fn workspace_opts(
        &self,
        settings: Settings,
        reuse: Reuse<'_>,
    ) -> WorkspaceResolveOptions {
        let mut opts = super::workspace_opts(settings, reuse);
        opts.peers.lockfile_dir = self.root.path().to_path_buf();
        opts
    }
}

fn write_manifest(root: &Path, id: &str, manifest: &Value) -> PackageManifest {
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("package.json");
    fs::write(&path, manifest.to_string()).unwrap();
    PackageManifest::from_path(path).unwrap()
}

fn catalogize(manifest: &mut Value, catalogs: &mut Catalogs) {
    for field in ["dependencies", "peerDependencies"] {
        let Some(deps) = manifest.get_mut(field).and_then(Value::as_object_mut) else { continue };
        for (alias, specifier) in deps {
            let range = specifier.as_str().unwrap().to_string();
            let catalog = match range.as_str() {
                "^1.0.0" => "one",
                "^2.0.0" => "two",
                "*" => "any",
                _ => panic!("unexpected generated range: {range}"),
            };
            catalogs
                .entry(catalog.to_string())
                .or_default()
                .insert(alias.clone(), range);
            *specifier = json!(format!("catalog:{catalog}"));
        }
    }
}
