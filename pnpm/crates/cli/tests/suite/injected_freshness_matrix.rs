//! An edit to an injected workspace package, checked across lockfile layouts,
//! injection forms, and filtered installs (pnpm/tasks#145).
//!
//! After the edit:
//!
//! - **Agreement.** `pnpm install --frozen-lockfile` from a clean tree fails
//!   exactly when a non-frozen install of the same selection changes a
//!   lockfile.
//! - **Convergence.** After that install, a frozen install from a clean tree
//!   passes.
//! - **Equivalence.** The lockfiles the install owns equal the ones a clean
//!   install without lockfiles writes. One edited project fails the frozen
//!   install of the whole workspace, so only this catches a stale lockfile of
//!   another project.

use crate::_utils::{
    ManifestDeps, WorkspaceFixture, assert_success, copy_sources, read_manifest,
    read_v11_bundle_path, set_dependency, walk_files, write_manifest_value,
};

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy)]
struct Layout {
    shared_lockfile: bool,
    injection: Injection,
}

/// How `app` declares its injected dependency on `lib`.
#[derive(Debug, Clone, Copy)]
enum Injection {
    InjectWorkspacePackages,
    DependenciesMeta,
    FileSpec,
}

#[derive(Debug, Clone, Copy)]
enum Edit {
    None,
    Dependency,
    Peer,
    OptionalPeer,
    ShadowedPeer,
    CatalogPeer,
}

struct Scenario {
    fixture: WorkspaceFixture,
    layout: Layout,
    copies: usize,
}

impl Scenario {
    /// `app` injects `lib`, and `lib` has a dependency and a peer from the
    /// catalog that `app` provides.
    fn new(layout: Layout) -> Self {
        let fixture = WorkspaceFixture::new();
        fixture.write_root_manifest("root", ManifestDeps::default());
        let scenario = Self { fixture, layout, copies: 0 };
        scenario.write_workspace_yaml("^100.0.0");
        scenario.write_manifest(
            "lib",
            json!({
                "dependencies": { "@pnpm.e2e/foo": "100.0.0" },
                "peerDependencies": { "@pnpm.e2e/bar": "catalog:" },
            }),
        );
        let (spec, meta) = match layout.injection {
            Injection::InjectWorkspacePackages => ("workspace:*", json!({})),
            Injection::DependenciesMeta => ("workspace:*", json!({ "lib": { "injected": true } })),
            Injection::FileSpec => ("file:../lib", json!({})),
        };
        scenario.write_manifest(
            "app",
            json!({
                "dependencies": {
                    "lib": spec,
                    "@pnpm.e2e/bar": "100.0.0",
                    "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0",
                },
                "dependenciesMeta": meta,
            }),
        );
        scenario
    }

    fn workspace(&self) -> &Path {
        &self.fixture.workspace
    }

    fn write_workspace_yaml(&self, bar_catalog_entry: &str) {
        fs::write(
            self.workspace().join("pnpm-workspace.yaml"),
            format!(
                "storeDir: {}\ncacheDir: {}\nenableGlobalVirtualStore: false\n\
                 packages:\n  - 'packages/*'\nsharedWorkspaceLockfile: {}\n\
                 injectWorkspacePackages: {}\ncatalog:\n  '@pnpm.e2e/bar': '{bar_catalog_entry}'\n",
                self.fixture.registry.store_dir.display(),
                self.fixture.registry.cache_dir.display(),
                self.layout.shared_lockfile,
                matches!(self.layout.injection, Injection::InjectWorkspacePackages),
            ),
        )
        .expect("write pnpm-workspace.yaml");
    }

    fn write_manifest(&self, project: &str, fields: Value) {
        let dir = self
            .workspace()
            .join("packages")
            .join(project);
        fs::create_dir_all(&dir).expect("create the project dir");
        let mut manifest = json!({ "name": project, "version": "1.0.0" });
        let Value::Object(fields) = fields else { panic!("the fields must be an object") };
        manifest
            .as_object_mut()
            .expect("a manifest object")
            .extend(fields);
        write_manifest_value(&dir, &manifest);
    }

    fn edit(&self, edit: Edit) {
        let lib = self.workspace().join("packages/lib");
        match edit {
            Edit::None => {}
            Edit::Dependency => set_dependency(&lib, "dependencies", "@pnpm.e2e/foo", "100.1.0"),
            Edit::Peer => {
                set_dependency(
                    &lib,
                    "peerDependencies",
                    "@pnpm.e2e/dep-of-pkg-with-1-dep",
                    "^100.0.0",
                );
            }
            Edit::OptionalPeer => {
                set_dependency(
                    &lib,
                    "peerDependencies",
                    "@pnpm.e2e/dep-of-pkg-with-1-dep",
                    "^100.0.0",
                );
                let mut manifest = read_manifest(&lib);
                manifest["peerDependenciesMeta"] =
                    json!({ "@pnpm.e2e/dep-of-pkg-with-1-dep": { "optional": true } });
                write_manifest_value(&lib, &manifest);
            }
            Edit::ShadowedPeer => {
                set_dependency(&lib, "dependencies", "@pnpm.e2e/dep-of-pkg-with-1-dep", "100.1.0");
                set_dependency(
                    &lib,
                    "peerDependencies",
                    "@pnpm.e2e/dep-of-pkg-with-1-dep",
                    "^100.0.0",
                );
            }
            Edit::CatalogPeer => self.write_workspace_yaml("100.0.0"),
        }
    }

    /// Every wanted lockfile under `workspace`.
    fn lockfiles(workspace: &Path) -> BTreeMap<String, String> {
        walk_files(workspace)
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name == "pnpm-lock.yaml")
                    && !path
                        .components()
                        .any(|part| part.as_os_str() == "node_modules")
            })
            .map(|path| {
                let relative = path.strip_prefix(workspace).expect("under the workspace");
                let text = fs::read_to_string(&path).expect("read a lockfile");
                (relative.to_string_lossy().replace('\\', "/"), text)
            })
            .collect()
    }

    /// A copy of the sources, without `node_modules`.
    fn clean_copy(&mut self) -> PathBuf {
        self.copies += 1;
        let copy = self
            .workspace()
            .with_file_name(format!("copy-{}", self.copies));
        copy_sources(self.workspace(), &copy);
        copy
    }

    fn install(&self, filter: &[&str]) {
        let output = self.fixture.command_at(
            self.workspace(),
            ["install", "--no-frozen-lockfile"].iter().chain(filter),
        );
        assert_success(&output);
    }

    /// Whether `pnpm install --frozen-lockfile` passes from a clean tree.
    /// Any failure other than an outdated lockfile or catalog fails the test.
    fn frozen_install_passes(&mut self, filter: &[&str]) -> bool {
        let copy = self.clean_copy();
        let output =
            self.fixture.command_at(&copy, ["install", "--frozen-lockfile"].iter().chain(filter));
        if output.status.success() {
            return true;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            ["ERR_PNPM_OUTDATED_LOCKFILE", "ERR_PNPM_LOCKFILE_CONFIG_MISMATCH"]
                .iter()
                .any(|code| stdout.contains(code) || stderr.contains(code)),
            "{:?}: the frozen install failed for another reason\nstdout:\n{stdout}\nstderr:\n{stderr}",
            self.layout,
        );
        false
    }

    /// The lockfiles a clean install without lockfiles writes.
    fn resolved_from_scratch(&mut self, v11_bundle: Option<&OsStr>) -> BTreeMap<String, String> {
        let copy = self.clean_copy();
        for lockfile in Self::lockfiles(&copy).keys() {
            fs::remove_file(copy.join(lockfile)).expect("remove a lockfile");
        }
        let output = if let Some(bundle) = v11_bundle {
            self.fixture.v11_command_at(bundle, &copy, ["install", "--no-frozen-lockfile"])
        } else {
            self.fixture.command_at(&copy, ["install", "--no-frozen-lockfile"])
        };
        assert_success(&output);
        Self::lockfiles(&copy)
    }
}

fn check(layout: Layout, edit: Edit, filter: &[&str]) {
    let mut scenario = Scenario::new(layout);
    scenario.install(&[]);
    scenario.edit(edit);

    let frozen_passed = scenario.frozen_install_passes(filter);
    let before = Scenario::lockfiles(scenario.workspace());
    scenario.install(filter);
    let after = Scenario::lockfiles(scenario.workspace());
    assert_eq!(
        frozen_passed,
        before == after,
        "{layout:?}, {edit:?}, filter {filter:?}: the frozen install passed: {frozen_passed}, \
         but the install changed the lockfiles: {}\nbefore: {before:#?}\nafter: {after:#?}",
        before != after,
    );

    scenario.install(filter);
    assert_eq!(
        Scenario::lockfiles(scenario.workspace()),
        after,
        "{layout:?}, {edit:?}, filter {filter:?}: a repeat install must keep the lockfiles byte-identical",
    );
    if filter.is_empty() {
        let run_gate = scenario.fixture.command_at(
            scenario.workspace(),
            ["--config.verify-deps-before-run=error", "exec", "node", "-e", ""],
        );
        assert_success(&run_gate);
    }

    assert!(
        scenario.frozen_install_passes(filter),
        "{layout:?}, {edit:?}, filter {filter:?}: a frozen install must pass after the install",
    );
    if filter.is_empty() && layout.shared_lockfile {
        let dedupe = scenario.fixture.command_at(scenario.workspace(), ["dedupe", "--check"]);
        assert_success(&dedupe);
    }
    let mut expected = scenario.resolved_from_scratch(None);
    let mut actual = after;
    if !filter.is_empty() {
        if layout.shared_lockfile {
            return;
        }
        expected.retain(|path, _| path == APP_LOCKFILE);
        actual.retain(|path, _| path == APP_LOCKFILE);
    }
    assert_eq!(
        actual, expected,
        "{layout:?}, {edit:?}, filter {filter:?}: the lockfiles differ from a resolution from scratch",
    );
    if filter.is_empty()
        && let Some(bundle) = read_v11_bundle_path()
    {
        let v11_expected = scenario.resolved_from_scratch(Some(&bundle));
        assert_eq!(
            actual, v11_expected,
            "{layout:?}, {edit:?}: the v12 lockfiles differ from a v11 resolution from scratch",
        );
    }
}

/// The lockfile `--filter app` owns when every project has its own. A
/// filtered install leaves a shared lockfile's other importers as they were,
/// so a resolution from scratch can't judge it.
const APP_LOCKFILE: &str = "packages/app/pnpm-lock.yaml";

macro_rules! matrix {
    ($($module:ident: $layout:expr,)*) => {
        $(
            mod $module {
                use super::{Edit, Injection, Layout, check};

                const LAYOUT: Layout = $layout;

                matrix!(@edits
                    no_edit: Edit::None,
                    a_dependency: Edit::Dependency,
                    a_peer: Edit::Peer,
                    an_optional_peer: Edit::OptionalPeer,
                    a_shadowed_peer: Edit::ShadowedPeer,
                    a_catalog_peer: Edit::CatalogPeer,
                );
            }
        )*
    };
    (@edits $($name:ident: $edit:expr,)*) => {
        $(
            mod $name {
                use super::{Edit, LAYOUT, check};

                #[test]
                fn full_install() {
                    check(LAYOUT, $edit, &[]);
                }

                #[test]
                fn filtered_install() {
                    check(LAYOUT, $edit, &["--filter", "app"]);
                }
            }
        )*
    };
}

matrix! {
    shared_inject_workspace_packages: Layout {
        shared_lockfile: true,
        injection: Injection::InjectWorkspacePackages,
    },
    shared_dependencies_meta: Layout {
        shared_lockfile: true,
        injection: Injection::DependenciesMeta,
    },
    shared_file_spec: Layout {
        shared_lockfile: true,
        injection: Injection::FileSpec,
    },
    dedicated_inject_workspace_packages: Layout {
        shared_lockfile: false,
        injection: Injection::InjectWorkspacePackages,
    },
    dedicated_dependencies_meta: Layout {
        shared_lockfile: false,
        injection: Injection::DependenciesMeta,
    },
    dedicated_file_spec: Layout {
        shared_lockfile: false,
        injection: Injection::FileSpec,
    },
}

/// When an injected workspace project sets `publishConfig.directory` and a
/// build writes a trimmed `package.json` into that directory, frozen and
/// non-frozen installs still judge freshness against the project's own
/// manifest across shared and dedicated lockfiles.
#[test]
fn publish_config_directory_with_built_manifest_agrees_across_layouts() {
    for shared_lockfile in [true, false] {
        let mut scenario = Scenario::new(Layout {
            shared_lockfile,
            injection: Injection::InjectWorkspacePackages,
        });
        let lib_dir = scenario.workspace().join("packages/lib");
        let dist_dir = lib_dir.join("dist");
        fs::create_dir_all(&dist_dir).unwrap();
        let mut lib_manifest = read_manifest(&lib_dir);
        lib_manifest["publishConfig"] = json!({ "directory": "dist" });
        write_manifest_value(&lib_dir, &lib_manifest);
        write_manifest_value(&dist_dir, &json!({ "name": "lib", "version": "1.0.0" }));

        scenario.install(&[]);
        assert!(scenario.frozen_install_passes(&[]));

        scenario.edit(Edit::Dependency);
        assert!(!scenario.frozen_install_passes(&[]));
        scenario.install(&[]);
        assert!(scenario.frozen_install_passes(&[]));
    }
}

/// A workspace `.pnpmfile.cjs` `readPackage` hook that rewrites an injected
/// package's dependencies is honored identically by resolution and frozen
/// verification across shared and dedicated lockfiles.
#[test]
fn pnpmfile_read_package_hook_agrees_across_layouts() {
    for shared_lockfile in [true, false] {
        let mut scenario = Scenario::new(Layout {
            shared_lockfile,
            injection: Injection::InjectWorkspacePackages,
        });
        fs::write(
            scenario.workspace().join(".pnpmfile.cjs"),
            "module.exports = { hooks: { readPackage(pkg) {\n\
               if (pkg.name === 'lib') {\n\
                 pkg.dependencies = { ...pkg.dependencies, '@pnpm.e2e/foo': '100.1.0' };\n\
               }\n\
               return pkg;\n\
             } } };\n",
        )
        .unwrap();
        scenario.install(&[]);
        assert!(scenario.frozen_install_passes(&[]));
    }
}
