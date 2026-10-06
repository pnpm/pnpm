//! Declared projects outside the lockfile dir, the shape Bit's capsule
//! installs take: the lockfile dir is one capsule, and the capsules dir
//! above it is a second project whose importer id is `..`.

use super::super::{Install, InstallError};
use pipe_trait::Pipe;
use pnpm_config::Config;
use pnpm_lockfile::{LazyLockfile, MaybeLazyLockfile};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::SilentReporter;
use pnpm_testing_utils::registry::TestRegistry;
use std::{
    fmt::Write,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tempfile::tempdir;

struct CapsuleLayout {
    root: PathBuf,
    capsule: PathBuf,
    capsules: PathBuf,
    config: &'static Config,
}

#[derive(Debug, Clone, Copy)]
enum Mode {
    Install,
    Frozen,
    Rebuild,
}

fn capsules(root: &Path, registry_url: &str) -> CapsuleLayout {
    let capsules = root.join("caps");
    let capsule = capsules.join("cap-a");
    let aspect = root.join("core/aspect");
    fs::create_dir_all(&capsule).unwrap();
    fs::create_dir_all(&aspect).unwrap();
    fs::write(
        aspect.join("package.json"),
        serde_json::json!({ "name": "core-aspect", "version": "1.0.0" }).to_string(),
    )
    .unwrap();

    let modules_dir = capsule.join("node_modules");
    let mut config = Config::new();
    config.cache_dir = root.join("cache");
    config.store_dir = root.join("store").into();
    config.install_state_dir = modules_dir.join(".pnpm");
    config.modules_dir = modules_dir;
    config.registry = registry_url.to_string();
    CapsuleLayout { root: root.to_path_buf(), capsule, capsules, config: config.leak() }
}

fn capsule_manifest(capsule: &Path) -> PackageManifest {
    PackageManifest::from_value(
        capsule.join("package.json"),
        serde_json::json!({
            "name": "cap-a",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        }),
    )
}

fn project(root_dir: &Path, manifest: serde_json::Value) -> pnpm_workspace::Project {
    pnpm_workspace::Project {
        root_dir: root_dir.to_path_buf(),
        manifest: PackageManifest::from_value(root_dir.join("package.json"), manifest),
        dependency_manifest: None,
    }
}

/// Install the way the NAPI binding does for a multi-project call. The
/// capsule and a project nested in it are always declared, so the
/// in-memory project list is used even when the capsules dir is not.
async fn run_capsule(
    setup: &CapsuleLayout,
    declare_capsules_dir: bool,
    mode: Mode,
) -> Result<(), InstallError> {
    let manifest = capsule_manifest(&setup.capsule);
    let nested = setup.capsule.join("nested");
    fs::create_dir_all(&nested).unwrap();
    let mut projects = vec![
        pnpm_workspace::Project {
            root_dir: setup.capsule.clone(),
            manifest: capsule_manifest(&setup.capsule),
            dependency_manifest: None,
        },
        project(&nested, serde_json::json!({ "name": "nested" })),
    ];
    if declare_capsules_dir {
        projects.push(project(
            &setup.capsules,
            serde_json::json!({ "dependencies": { "core-aspect": "link:../core/aspect" } }),
        ));
    }
    let lazy_lockfile =
        LazyLockfile::deferred(setup.capsule.clone(), setup.config.wanted_lockfile_selection());
    let lockfile_path = setup.capsule.join(setup.config.wanted_lockfile_name());
    let resolved_packages = Default::default();
    let http_client: Arc<pnpm_network::ThrottledClient> = Arc::default();
    let install = Install::new(
        Arc::default(),
        &resolved_packages,
        (&http_client, Arc::clone(&http_client)),
        setup.config,
        &manifest,
        MaybeLazyLockfile::Lazy(&lazy_lockfile),
        vec![DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
    );
    Install {
        lockfile_policy: crate::InstallLockfilePolicy {
            manifest_freshness: crate::ManifestFreshness::Content,
            frozen: !matches!(mode, Mode::Install),
            ..install.lockfile_policy
        },
        context: crate::InstallInvocation {
            lockfile_path: Some(&lockfile_path),
            ..install.context
        },
        projects: crate::InstallProjects {
            workspace_projects_override: Some(projects),
            ..install.projects
        },
        ..install
    }
    .pipe(|install| async move {
        match mode {
            Mode::Rebuild => {
                install.run_rebuild::<SilentReporter>(pnpm_deps_restorer::RebuildOptions {
                    selected_names: None,
                    pending_projects: Vec::new(),
                    check_lockfile_patches_only: false,
                })
                .await
            }
            Mode::Install | Mode::Frozen => install.run::<SilentReporter>().await,
        }
    })
    .await
}

fn assert_capsules_dir_linked(setup: &CapsuleLayout) {
    let link = setup.capsules.join("node_modules/core-aspect");
    assert!(
        fs::symlink_metadata(&link).is_ok_and(|meta| meta.file_type().is_symlink()),
        "{link:?} should be a symlink",
    );
    assert!(link.join("package.json").exists());
}

fn read_lockfile(setup: &CapsuleLayout) -> String {
    fs::read_to_string(setup.capsule.join("pnpm-lock.yaml")).unwrap()
}

/// A lockfile that records the `..` importer, written by an install that
/// declared the capsules dir.
async fn lockfile_with_capsules_dir_importer(registry_url: &str) -> String {
    let dir = tempdir().unwrap();
    let setup = capsules(dir.path(), registry_url);
    run_capsule(&setup, true, Mode::Install).await.expect("install writing the lockfile");
    let lockfile = read_lockfile(&setup);
    assert!(lockfile.contains("\n  ..:\n"), "lockfile has the `..` importer:\n{lockfile}");
    lockfile
}

#[tokio::test]
async fn declared_project_above_the_lockfile_dir_installs_on_every_path() {
    let registry = TestRegistry::start();
    let dir = tempdir().unwrap();
    let setup = capsules(dir.path(), registry.url());

    run_capsule(&setup, true, Mode::Install).await.expect("first install");
    assert!(read_lockfile(&setup).contains("\n  ..:\n"));
    assert_capsules_dir_linked(&setup);

    run_capsule(&setup, true, Mode::Install).await.expect("repeat install");
    assert_capsules_dir_linked(&setup);

    for mode in [Mode::Frozen, Mode::Rebuild] {
        fs::remove_dir_all(setup.capsule.join("node_modules")).unwrap();
        fs::remove_dir_all(setup.capsules.join("node_modules")).unwrap();
        run_capsule(&setup, true, mode).await.unwrap_or_else(|error| panic!("{mode:?}: {error}"));
        assert_capsules_dir_linked(&setup);
    }
}

#[tokio::test]
async fn declared_project_above_the_lockfile_dir_installs_from_a_copied_lockfile() {
    let registry = TestRegistry::start();
    let lockfile = lockfile_with_capsules_dir_importer(registry.url()).await;
    for mode in [Mode::Install, Mode::Frozen] {
        let dir = tempdir().unwrap();
        let setup = capsules(dir.path(), registry.url());
        fs::write(setup.capsule.join("pnpm-lock.yaml"), &lockfile).unwrap();

        run_capsule(&setup, true, mode).await.unwrap_or_else(|error| panic!("{mode:?}: {error}"));
        assert_capsules_dir_linked(&setup);
        run_capsule(&setup, true, mode).await
            .unwrap_or_else(|error| panic!("{mode:?} repeat: {error}"));
        assert_capsules_dir_linked(&setup);
    }
}

/// Bit declares the capsules dir for one capsule per run, so a lockfile
/// copied from that capsule reaches others that do not declare it.
#[tokio::test]
async fn undeclared_importer_above_the_lockfile_dir_is_skipped() {
    let registry = TestRegistry::start();
    let lockfile = lockfile_with_capsules_dir_importer(registry.url()).await;
    for mode in [Mode::Install, Mode::Frozen, Mode::Rebuild] {
        let dir = tempdir().unwrap();
        let setup = capsules(dir.path(), registry.url());
        fs::write(setup.capsule.join("pnpm-lock.yaml"), &lockfile).unwrap();
        if matches!(mode, Mode::Rebuild) {
            run_capsule(&setup, true, Mode::Install).await.expect("install before the rebuild");
            fs::remove_dir_all(setup.capsules.join("node_modules")).unwrap();
        }

        run_capsule(&setup, false, mode).await.unwrap_or_else(|error| panic!("{mode:?}: {error}"));
        assert!(
            !setup.capsules.join("node_modules").exists(),
            "{mode:?} wrote into the undeclared capsules dir",
        );
    }
}

/// Lockfile-only importer keys outside the lockfile dir stay untrusted
/// even when the install declares `..`.
#[tokio::test]
async fn lockfile_only_escaping_importers_are_never_linked() {
    let registry = TestRegistry::start();
    let lockfile = lockfile_with_capsules_dir_importer(registry.url()).await;
    for mode in [Mode::Install, Mode::Frozen] {
        install_with_escaping_importers(registry.url(), &lockfile, mode).await;
    }
}

async fn install_with_escaping_importers(registry_url: &str, lockfile: &str, mode: Mode) {
    let dir = tempdir().unwrap();
    let setup = capsules(dir.path(), registry_url);
    let absolute = setup.root.join("abs");
    let hostile_keys = [
        "../other".to_string(),
        "../../etc".to_string(),
        absolute.display().to_string(),
        "C:/x".to_string(),
    ];
    fs::write(
        setup.capsule.join("pnpm-lock.yaml"),
        with_importers_like_dotdot(lockfile, &hostile_keys),
    )
    .unwrap();

    let result = run_capsule(&setup, true, mode).await;
    eprintln!("{mode:?}: {result:?}");
    if let Err(error) = &result {
        let code = miette::Diagnostic::code(error).map(|code| code.to_string());
        assert_eq!(
            code.as_deref(),
            Some("ERR_PNPM_PACKAGE_MANAGER_UNSAFE_IMPORTER_PATH"),
            "{mode:?} failed with an unexpected error: {error}",
        );
    }
    for escaped in [
        setup.capsules.join("other/node_modules"),
        setup.root.join("etc/node_modules"),
        absolute.join("node_modules"),
        setup.capsule.join("C:/x/node_modules"),
    ] {
        assert!(!escaped.exists(), "{mode:?} wrote {escaped:?}");
    }
    if matches!(mode, Mode::Frozen) {
        let on_disk = read_lockfile(&setup);
        let unread: Vec<_> = hostile_keys
            .iter()
            .filter(|key| !on_disk.contains(&format!("\n  {key:?}:\n")))
            .collect();
        assert!(unread.is_empty(), "the frozen install dropped {unread:?}");
    }
}

/// `lockfile` with an importer per key, each a copy of the `..` importer.
fn with_importers_like_dotdot(lockfile: &str, keys: &[String]) -> String {
    let dotdot_block = lockfile
        .split("\n  ..:\n")
        .nth(1)
        .unwrap()
        .split("\n\n")
        .next()
        .unwrap();
    let extra_importers = keys
        .iter()
        .fold(String::new(), |mut acc, key| {
            write!(acc, "\n  {key:?}:\n{dotdot_block}\n").unwrap();
            acc
        });
    lockfile.replacen("\n  ..:\n", &format!("{extra_importers}\n  ..:\n"), 1)
}
