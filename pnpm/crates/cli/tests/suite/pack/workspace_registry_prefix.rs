//! The named-registry prefix a `workspace:` specifier carries in a packed
//! manifest: that of the registry the target package publishes to. A
//! consumer that installs the package through the prefix and a sibling that
//! depends on it through its published manifest then record one lockfile
//! key for it.

use super::read_manifest_from_tarball;
use crate::_utils::{
    hosted_registry::{add_user, spawn_hosted_registry, write_auth_file},
    pacquet_in, read_lockfile,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::PackageKey;
use pnpm_testing_utils::bin::CommandTempCwd;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{collections::HashMap, fs, path::Path, process::Command};

/// The registry every prefixed sibling publishes to.
const WORK_REGISTRY: &str = "https://npm.work.example.com/";

/// The `registries` shape: the `work` prefix and the `@work` scope route on
/// one entry.
const REGISTRIES_WITH_PREFIX: &str = "registries:
  https://npm.work.example.com/:
    prefix: work
    scopes:
      - '@work'
";

/// The same registry without a prefix.
const REGISTRIES_WITHOUT_PREFIX: &str = "registries:
  https://npm.work.example.com/:
    scopes:
      - '@work'
";

/// The deprecated `namedRegistries` shape next to a scope route, spelled the
/// way a registry URL is matched: the prefix's URL differs from the publish
/// URL in host case and trailing slash.
const NAMED_REGISTRIES_WITH_PREFIX: &str = "namedRegistries:
  work: https://NPM.Work.Example.com
registries:
  '@work': https://npm.work.example.com/
";

/// Write `pnpm-workspace.yaml` from `settings` plus the `packages` glob, and
/// one `packages/<dir>/package.json` per manifest.
fn write_workspace(workspace: &Path, settings: &str, manifests: &[(&str, Value)]) {
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("packages:\n  - packages/*\ncatalog:\n  pkg-e: workspace:5.0.0\n{settings}"),
    )
    .expect("write pnpm-workspace.yaml");
    for (dir, manifest) in manifests {
        let dir = workspace.join("packages").join(dir);
        fs::create_dir_all(&dir).expect("create package dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }
}

/// The siblings `pkg-main` depends on, each reaching [`WORK_REGISTRY`] by a
/// route `pnpm publish` honors: the `@work` scope, `publishConfig.registry`
/// and `publishConfig["@other:registry"]`. `pkg-c` publishes to the default
/// registry.
fn siblings() -> Vec<(&'static str, Value)> {
    vec![
        ("a", json!({ "name": "@work/a", "version": "1.0.0" })),
        (
            "b",
            json!({
                "name": "pkg-b",
                "version": "2.0.0",
                "publishConfig": { "registry": WORK_REGISTRY },
            }),
        ),
        ("c", json!({ "name": "pkg-c", "version": "3.0.0" })),
        (
            "d",
            json!({
                "name": "@other/d",
                "version": "4.0.0",
                "publishConfig": { "@other:registry": WORK_REGISTRY },
            }),
        ),
        (
            "e",
            json!({
                "name": "pkg-e",
                "version": "5.0.0",
                "publishConfig": { "registry": WORK_REGISTRY },
            }),
        ),
    ]
}

/// Every `workspace:` form the rewrite accepts, spread over the dependency
/// fields. `pkg-e` reaches its `workspace:5.0.0` through the catalog.
fn main_manifest() -> Value {
    json!({
        "name": "pkg-main",
        "version": "0.1.0",
        "dependencies": {
            "@work/a": "workspace:*",
            "pkg-b": "workspace:^",
            "alias-b": "workspace:pkg-b@~",
            "pkg-c": "workspace:*",
        },
        "devDependencies": { "@other/d": "workspace:~" },
        "optionalDependencies": { "pkg-e": "catalog:" },
        "peerDependencies": { "@work/a": "workspace:^" },
    })
}

fn workspace_with_main(workspace: &Path, settings: &str, main: Value) {
    let mut manifests = siblings();
    manifests.push(("main", main));
    write_workspace(workspace, settings, &manifests);
}

/// Pack `pkg-main` from the workspace root into `<workspace>/out` and read
/// the manifest the tarball carries.
fn pack_main(workspace: &Path) -> Value {
    pacquet_in(workspace)
        .with_args(["--filter", "pkg-main", "pack", "--pack-destination"])
        .with_arg(workspace.join("out"))
        .assert()
        .success();
    read_manifest_from_tarball(&workspace.join("out/pkg-main-0.1.0.tgz"))
}

/// What `pkg-main` publishes with the `work` prefix declared. A peer range
/// stays a plain semver range: the consumer matches it against the installed
/// version with semver, which a prefix is not part of.
fn prefixed_fields() -> Value {
    json!({
        "dependencies": {
            "@work/a": "work:1.0.0",
            "pkg-b": "work:^2.0.0",
            "alias-b": "work:pkg-b@~2.0.0",
            "pkg-c": "3.0.0",
        },
        "devDependencies": { "@other/d": "work:~4.0.0" },
        "optionalDependencies": { "pkg-e": "work:5.0.0" },
        "peerDependencies": { "@work/a": "^1.0.0" },
    })
}

/// What `pkg-main` publishes when no prefix points at the registry.
fn plain_fields() -> Value {
    json!({
        "dependencies": {
            "@work/a": "1.0.0",
            "pkg-b": "^2.0.0",
            "alias-b": "npm:pkg-b@~2.0.0",
            "pkg-c": "3.0.0",
        },
        "devDependencies": { "@other/d": "~4.0.0" },
        "optionalDependencies": { "pkg-e": "5.0.0" },
        "peerDependencies": { "@work/a": "^1.0.0" },
    })
}

fn dependency_fields(manifest: &Value) -> Value {
    let mut fields = serde_json::Map::new();
    for field in ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"] {
        fields.insert(field.to_string(), manifest[field].clone());
    }
    Value::Object(fields)
}

#[test]
fn packed_workspace_specifiers_carry_the_prefix_declared_in_registries() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    workspace_with_main(&workspace, REGISTRIES_WITH_PREFIX, main_manifest());

    let manifest = pack_main(&workspace);

    assert_eq!(dependency_fields(&manifest), prefixed_fields());
    drop(root);
}

#[test]
fn packed_workspace_specifiers_carry_the_prefix_declared_in_named_registries() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    workspace_with_main(&workspace, NAMED_REGISTRIES_WITH_PREFIX, main_manifest());

    let manifest = pack_main(&workspace);

    assert_eq!(dependency_fields(&manifest), prefixed_fields());
    drop(root);
}

#[test]
fn packed_workspace_specifiers_stay_plain_without_a_prefix() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    workspace_with_main(&workspace, REGISTRIES_WITHOUT_PREFIX, main_manifest());

    let manifest = pack_main(&workspace);

    assert_eq!(dependency_fields(&manifest), plain_fields());
    drop(root);
}

/// Several prefixes on one URL: the first in alphabetical order is used.
#[test]
fn the_alphabetically_first_of_several_prefixes_on_one_registry_is_used() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let settings = "namedRegistries:
  zeta: https://npm.work.example.com/
  alpha: https://npm.work.example.com/
registries:
  '@work': https://npm.work.example.com/
";
    workspace_with_main(&workspace, settings, main_manifest());

    let manifest = pack_main(&workspace);

    assert_eq!(manifest["dependencies"]["@work/a"], "alpha:1.0.0");
    assert_eq!(manifest["dependencies"]["alias-b"], "alpha:pkg-b@~2.0.0");
    drop(root);
}

#[test]
fn pack_inside_the_package_directory_carries_the_prefix() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    workspace_with_main(&workspace, REGISTRIES_WITH_PREFIX, main_manifest());
    let main_dir = workspace.join("packages/main");

    pacquet_in(&main_dir)
        .with_arg("pack")
        .assert()
        .success();

    let manifest = read_manifest_from_tarball(&main_dir.join("pkg-main-0.1.0.tgz"));
    assert_eq!(dependency_fields(&manifest), prefixed_fields());
    drop(root);
}

/// `publishConfig.directory` packs the manifest of another directory, whose
/// `workspace:` specifiers are rewritten the same way.
#[test]
fn publish_config_directory_manifest_carries_the_prefix() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut main = main_manifest();
    main["publishConfig"] = json!({ "directory": "dist" });
    workspace_with_main(&workspace, REGISTRIES_WITH_PREFIX, main);
    let dist = workspace.join("packages/main/dist");
    fs::create_dir_all(&dist).expect("create dist");
    fs::write(dist.join("package.json"), main_manifest().to_string()).expect("write dist manifest");

    let manifest = pack_main(&workspace);

    assert_eq!(dependency_fields(&manifest), prefixed_fields());
    drop(root);
}

/// A sibling that publishes from `publishConfig.directory` is published with
/// that directory's manifest, so that manifest's registry decides the prefix.
#[test]
fn a_sibling_publishing_from_a_directory_takes_the_registry_of_that_manifest() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let publishes_from_dist = |name: &str, version: &str, registry: Option<&str>| {
        let mut publish_config = json!({ "directory": "dist" });
        if let Some(registry) = registry {
            publish_config["registry"] = json!(registry);
        }
        json!({ "name": name, "version": version, "publishConfig": publish_config })
    };
    let main = json!({
        "name": "pkg-main",
        "version": "0.1.0",
        "dependencies": { "pkg-f": "workspace:*", "pkg-g": "workspace:^" },
    });
    write_workspace(
        &workspace,
        REGISTRIES_WITH_PREFIX,
        &[
            ("f", publishes_from_dist("pkg-f", "6.0.0", Some(WORK_REGISTRY))),
            ("g", publishes_from_dist("pkg-g", "7.0.0", None)),
            ("main", main),
        ],
    );
    for (dir, manifest) in [
        (
            "f",
            json!({ "name": "pkg-f", "version": "6.0.0", "publishConfig": { "registry": "https://npm.other.example.com/" } }),
        ),
        (
            "g",
            json!({ "name": "pkg-g", "version": "7.0.0", "publishConfig": { "registry": WORK_REGISTRY } }),
        ),
    ] {
        let dist = workspace
            .join("packages")
            .join(dir)
            .join("dist");
        fs::create_dir_all(&dist).expect("create dist");
        fs::write(dist.join("package.json"), manifest.to_string()).expect("write dist manifest");
    }

    let manifest = pack_main(&workspace);

    assert_eq!(manifest["dependencies"], json!({ "pkg-f": "6.0.0", "pkg-g": "work:^7.0.0" }));
    drop(root);
}

/// Two projects of one name publish through their own registries; an
/// explicit version names which one a specifier means.
#[test]
fn same_name_siblings_publish_through_their_own_registries() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let main = json!({
        "name": "pkg-main",
        "version": "0.1.0",
        "dependencies": { "dup": "workspace:2.0.0", "dup-old": "workspace:dup@1.0.0" },
    });
    write_workspace(
        &workspace,
        REGISTRIES_WITH_PREFIX,
        &[
            (
                "dup-1",
                json!({
                    "name": "dup",
                    "version": "1.0.0",
                    "publishConfig": { "registry": "https://npm.other.example.com/" },
                }),
            ),
            (
                "dup-2",
                json!({
                    "name": "dup",
                    "version": "2.0.0",
                    "publishConfig": { "registry": WORK_REGISTRY },
                }),
            ),
            ("main", main),
        ],
    );

    let manifest = pack_main(&workspace);

    assert_eq!(
        manifest["dependencies"],
        json!({ "dup": "work:2.0.0", "dup-old": "npm:dup@1.0.0" }),
    );
    drop(root);
}

/// Clear the CI / OIDC environment so a spawned publish never attempts an
/// id-token exchange.
fn clear_ci(command: Command) -> Command {
    command
        .without_env("GITHUB_ACTIONS")
        .without_env("GITLAB_CI")
        .without_env("NPM_ID_TOKEN")
        .without_env("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
        .without_env("ACTIONS_ID_TOKEN_REQUEST_URL")
}

/// `.npmrc` pointing a project at `registry`, with a store of the test's own.
fn write_npmrc(dir: &Path, registry: &str) {
    fs::write(
        dir.join(".npmrc"),
        format!("registry={registry}\nstore-dir=../store\ncache-dir=../cache\n"),
    )
    .expect("write .npmrc");
}

/// The lockfile keys of `name` in `entries`, sorted.
fn keys_of<Entry>(entries: Option<&HashMap<PackageKey, Entry>>, name: &str) -> Vec<String> {
    let mut keys: Vec<String> = entries
        .into_iter()
        .flatten()
        .map(|(key, _)| key.to_string())
        .filter(|key| key.starts_with(&format!("{name}@")))
        .collect();
    keys.sort();
    keys
}

/// The whole point, end to end: `b` depends on `a` through `workspace:*`, both
/// publish to the registry the `work` prefix names, and a consumer installing
/// `a` and `b` through the prefix holds one lockfile entry for `a`.
#[test]
fn published_siblings_share_one_lockfile_entry_with_a_prefixed_consumer() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (registry, _storage) = spawn_hosted_registry();
    let token = add_user(&registry);
    let auth_file = root.path().join("auth-npmrc");
    write_auth_file(&auth_file, &registry, &token);
    let settings = format!("registries:\n  \"{registry}\":\n    prefix: work\n");

    write_workspace(
        &workspace,
        &settings,
        &[
            ("a", json!({ "name": "e2e-prefix-a", "version": "1.0.0" })),
            (
                "b",
                json!({
                    "name": "e2e-prefix-b",
                    "version": "1.0.0",
                    "dependencies": { "e2e-prefix-a": "workspace:*" },
                }),
            ),
        ],
    );
    write_npmrc(&workspace, &registry);
    clear_ci(pacquet_in(&workspace))
        .with_args(["--npmrc-auth-file"])
        .with_arg(&auth_file)
        .with_args(["-r", "publish", "--no-git-checks"])
        .assert()
        .success();

    let consumer = root.path().join("consumer");
    fs::create_dir_all(&consumer).expect("create consumer");
    fs::write(
        consumer.join("package.json"),
        json!({
            "name": "consumer",
            "version": "1.0.0",
            "dependencies": {
                "e2e-prefix-a": "work:^1.0.0",
                "e2e-prefix-b": "work:^1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write consumer manifest");
    fs::write(consumer.join("pnpm-workspace.yaml"), &settings)
        .expect("write consumer workspace yaml");
    write_npmrc(&consumer, &registry);
    pacquet_in(&consumer)
        .with_args(["--npmrc-auth-file"])
        .with_arg(&auth_file)
        .with_arg("install")
        .assert()
        .success();

    let lockfile = read_lockfile(&consumer.join("pnpm-lock.yaml"));
    assert_eq!(keys_of(lockfile.packages.as_ref(), "e2e-prefix-a"), ["e2e-prefix-a@work:1.0.0"]);
    assert_eq!(keys_of(lockfile.snapshots.as_ref(), "e2e-prefix-a"), ["e2e-prefix-a@work:1.0.0"]);
    drop(root);
}
