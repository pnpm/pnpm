//! Global groups link their packages from the global virtual store at
//! `<storeDir>/links` unless `virtualStoreType` says otherwise.

use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, Path, PathBuf, fs, global_command,
    prepare_global_home,
};
use assert_cmd::assert::OutputAssertExt;
use pnpm_store_dir::STORE_VERSION;

const PARENT: &str = "@pnpm.e2e/parent-of-pkg-with-1-dep";
const PKG: &str = "@pnpm.e2e/pkg-with-1-dep";
const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";

fn links_dir(store_dir: &Path) -> PathBuf {
    store_dir.join(STORE_VERSION).join("links")
}

/// The hash directories of every version of `name` in the global virtual
/// store, sorted.
fn slots(store_dir: &Path, name: &str) -> Vec<PathBuf> {
    let name_dir = pnpm_fs::join_slash_separated_path(&links_dir(store_dir), name);
    let Ok(versions) = fs::read_dir(&name_dir) else { return Vec::new() };
    let mut slots: Vec<PathBuf> = versions
        .flatten()
        .flat_map(|version| {
            fs::read_dir(version.path())
                .into_iter()
                .flatten()
                .flatten()
        })
        .map(|hash| hash.path())
        .collect();
    slots.sort();
    slots
}

/// Whether the group's own `node_modules/.pnpm` holds a copy of `name`.
fn has_local_slot(install_dir: &Path, name: &str) -> bool {
    let prefix = format!("{}@", name.replace('/', "+"));
    fs::read_dir(install_dir.join("node_modules/.pnpm"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&prefix)
        })
}

/// [`prepare_global_home`] with the store spelled canonically, like the pnpm
/// home [`global_command`] passes. The store's project registry links each
/// group by a relative path, which only resolves when both ends use the same
/// spelling of a symlinked ancestor such as macOS's `/var`.
fn prepare_canonical_global_home(pnpm_home: &Path, npmrc_info: &AddMockedRegistry) {
    prepare_global_home(pnpm_home, npmrc_info);
    fs::create_dir_all(&npmrc_info.store_dir).expect("create the store dir");
    let store_dir = fs::canonicalize(&npmrc_info.store_dir).expect("resolve the store dir");
    let workspace_yaml = pnpm_home.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&workspace_yaml).expect("read the pnpm-home workspace yaml");
    let yaml = yaml.replace(
        &format!("storeDir: {}", npmrc_info.store_dir.display()),
        &format!("storeDir: {}", store_dir.display()),
    );
    fs::write(&workspace_yaml, yaml).expect("write the pnpm-home workspace yaml");
}

fn global_add(workspace: &Path, pnpm_home: &Path, package: &str) {
    global_command(workspace, pnpm_home)
        .with_args(["add", "-g", package])
        .assert()
        .success();
}

fn install_dir_of(pnpm_home: &Path, package: &str) -> PathBuf {
    let global_pkg_dir = pnpm_home.join("global").join(STORE_VERSION);
    fs::read_dir(&global_pkg_dir)
        .expect("read the global packages dir")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_symlink())
        .map(|link| fs::canonicalize(link).expect("resolve the hash link"))
        .find(|install_dir| {
            pnpm_fs::join_slash_separated_path(&install_dir.join("node_modules"), package).exists()
        })
        .unwrap_or_else(|| panic!("no global group installs {package}"))
}

#[test]
fn global_add_links_packages_from_the_global_virtual_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    prepare_canonical_global_home(&pnpm_home, &npmrc_info);

    global_add(&workspace, &pnpm_home, PKG);

    let pkg_slots = slots(&npmrc_info.store_dir, PKG);
    assert_eq!(pkg_slots.len(), 1, "one slot for {PKG}: {pkg_slots:?}");
    assert_eq!(slots(&npmrc_info.store_dir, DEP).len(), 1, "one slot for {DEP}");
    let install_dir = install_dir_of(&pnpm_home, PKG);
    let linked = fs::canonicalize(pnpm_fs::join_slash_separated_path(
        &install_dir.join("node_modules"),
        PKG,
    ))
    .expect("resolve the group's direct dependency");
    assert!(
        linked.starts_with(fs::canonicalize(&pkg_slots[0]).expect("resolve the slot")),
        "the group's {PKG} must resolve into the global virtual store, got {linked:?}",
    );
    assert!(!has_local_slot(&install_dir, PKG), "the group must not keep its own copy of {PKG}");

    drop((root, npmrc_info));
}

#[test]
fn global_groups_share_the_slots_of_a_common_dependency() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    prepare_canonical_global_home(&pnpm_home, &npmrc_info);

    global_add(&workspace, &pnpm_home, PKG);
    let pkg_slots = slots(&npmrc_info.store_dir, PKG);
    global_add(&workspace, &pnpm_home, PARENT);

    assert_eq!(
        slots(&npmrc_info.store_dir, PKG),
        pkg_slots,
        "the second group must reuse the first group's {PKG} slot",
    );
    assert_eq!(slots(&npmrc_info.store_dir, DEP).len(), 1, "one slot for {DEP}");
    assert_eq!(slots(&npmrc_info.store_dir, PARENT).len(), 1, "one slot for {PARENT}");
    assert_ne!(
        install_dir_of(&pnpm_home, PKG),
        install_dir_of(&pnpm_home, PARENT),
        "the packages must be installed as separate groups",
    );

    drop((root, npmrc_info));
}

/// Where a user can set `virtualStoreType: project` for global installs.
#[derive(Debug, Clone, Copy)]
enum OptOut {
    PnpmHomeYaml,
    Env,
    Cli,
}

#[test]
fn global_add_keeps_a_group_local_virtual_store_when_the_project_type_is_set() {
    for opt_out in [OptOut::PnpmHomeYaml, OptOut::Env, OptOut::Cli] {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let pnpm_home = root.path().join("pnpm-home");
        prepare_canonical_global_home(&pnpm_home, &npmrc_info);
        let mut command = global_command(&workspace, &pnpm_home).with_args(["add", "-g", PKG]);
        match opt_out {
            OptOut::PnpmHomeYaml => {
                let workspace_yaml = pnpm_home.join("pnpm-workspace.yaml");
                let mut yaml =
                    fs::read_to_string(&workspace_yaml).expect("read the pnpm-home workspace yaml");
                yaml.push_str("virtualStoreType: project\n");
                fs::write(&workspace_yaml, yaml).expect("write the pnpm-home workspace yaml");
            }
            OptOut::Env => command = command.with_env("PNPM_CONFIG_VIRTUAL_STORE_TYPE", "project"),
            OptOut::Cli => command = command.with_arg("--config.virtual-store-type=project"),
        }

        command.assert().success();

        let install_dir = install_dir_of(&pnpm_home, PKG);
        let linked = fs::canonicalize(pnpm_fs::join_slash_separated_path(
            &install_dir.join("node_modules"),
            PKG,
        ))
        .expect("resolve the group's direct dependency");
        assert!(
            linked.starts_with(&install_dir),
            "{opt_out:?}: {PKG} must resolve inside the group, got {linked:?}",
        );
        assert!(
            has_local_slot(&install_dir, PKG),
            "{opt_out:?}: the group must keep its own virtual store",
        );

        drop((root, npmrc_info));
    }
}

#[test]
fn store_prune_after_global_remove_keeps_only_the_slots_other_groups_use() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    prepare_canonical_global_home(&pnpm_home, &npmrc_info);
    global_add(&workspace, &pnpm_home, PKG);
    global_add(&workspace, &pnpm_home, PARENT);
    let pkg_slots = slots(&npmrc_info.store_dir, PKG);
    let dep_slots = slots(&npmrc_info.store_dir, DEP);
    assert_eq!(slots(&npmrc_info.store_dir, PARENT).len(), 1, "one slot for {PARENT}");

    global_command(&workspace, &pnpm_home)
        .with_args(["remove", "-g", PARENT])
        .assert()
        .success();
    let prune = global_command(&workspace, &pnpm_home)
        .with_args(["store", "prune"])
        .output()
        .expect("run store prune");
    let stderr = String::from_utf8_lossy(&prune.stderr);
    assert!(prune.status.success(), "store prune failed: {stderr}");

    assert!(
        slots(&npmrc_info.store_dir, PARENT).is_empty(),
        "prune must remove {PARENT}'s slot: {stderr}",
    );
    assert_eq!(slots(&npmrc_info.store_dir, PKG), pkg_slots, "prune must keep {PKG}'s slot");
    assert_eq!(slots(&npmrc_info.store_dir, DEP), dep_slots, "prune must keep {DEP}'s slot");

    drop((root, npmrc_info));
}

/// A global install reads no settings from the project it runs in, so a
/// project cannot move the global groups' packages into a directory it
/// controls.
#[test]
fn global_add_ignores_the_caller_project_virtual_store_dirs() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    prepare_canonical_global_home(&pnpm_home, &npmrc_info);
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read the project workspace yaml");
    yaml.push_str("globalVirtualStoreDir: project-gvs\nvirtualStoreDir: project-vs\n");
    fs::write(&workspace_yaml, yaml).expect("write the project workspace yaml");

    global_add(&workspace, &pnpm_home, PKG);

    assert_eq!(slots(&npmrc_info.store_dir, PKG).len(), 1, "one slot for {PKG} in the store");
    assert!(
        !workspace.join("project-gvs").exists(),
        "the project's globalVirtualStoreDir is unused",
    );
    assert!(!workspace.join("project-vs").exists(), "the project's virtualStoreDir is unused");

    drop((root, npmrc_info));
}
