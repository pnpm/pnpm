//! Resolution tests for the local-filesystem resolver, one
//! `#[tokio::test]` per scenario.

use pnpm_lockfile::{LockfileResolution, TarballResolution};
use pnpm_resolving_local_resolver::{
    LocalResolverContext, LocalResolverOptions, LocalResolverUpdate, ResolveLocalError,
    WantedLocalDependency, resolve_from_local_path, resolve_from_local_scheme,
};
use pnpm_resolving_resolver_base::PkgResolutionId;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

/// Set up a `<tmp>/inner/` directory with a package.json carrying the
/// `name` the tests assert against. Returns `(tmp, inner)` so the temp
/// dir lives as long as the test.
fn fixture() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let inner = tmp.path().join("inner");
    fs::create_dir_all(&inner).expect("create inner dir");
    fs::write(
        tmp.path().join("package.json"),
        r#"{"name":"@pnpm/resolving.local-resolver","version":"0.0.0"}"#,
    )
    .expect("write package.json");
    (tmp, inner)
}

fn opts(project_dir: &Path) -> LocalResolverOptions {
    LocalResolverOptions {
        project_dir: project_dir.to_path_buf(),
        lockfile_dir: None,
        current_pkg: None,
        update: LocalResolverUpdate::Off,
    }
}

fn ctx_default() -> LocalResolverContext {
    LocalResolverContext::default()
}

#[tokio::test]
async fn resolve_directory() {
    let (_tmp, project_dir) = fixture();
    let wd = WantedLocalDependency { bare_specifier: "..".to_string(), injected: false };

    let result = resolve_from_local_path(&ctx_default(), &wd, &opts(&project_dir))
        .await
        .expect("resolve")
        .expect("claims");

    assert_eq!(result.id.as_str(), "link:..");
    assert_eq!(result.normalized_bare_specifier.as_deref(), Some("link:.."));
    let manifest = result.manifest.as_ref().expect("manifest");
    assert_eq!(
        manifest.get("name").and_then(|value| value.as_str()),
        Some("@pnpm/resolving.local-resolver"),
    );
    let LockfileResolution::Directory(dir) = &result.resolution else {
        panic!("expected directory resolution, got {:?}", result.resolution);
    };
    let expected_dir =
        forward_slashes(project_dir.join("..").lexical_normalize().display().to_string());
    assert_eq!(dir.directory, expected_dir);
}

#[tokio::test]
async fn resolve_directory_specified_using_absolute_path() {
    let (_tmp, project_dir) = fixture();
    let linked_dir = project_dir.join("..").lexical_normalize();
    let normalized_linked_dir = forward_slashes(linked_dir.display().to_string());

    let wd = WantedLocalDependency {
        bare_specifier: format!("link:{}", linked_dir.display()),
        injected: false,
    };
    let result = resolve_from_local_scheme(&ctx_default(), &wd, &opts(&project_dir))
        .await
        .expect("resolve")
        .expect("claims");

    assert_eq!(result.id.as_str(), "link:..");
    assert_eq!(
        result.normalized_bare_specifier.as_deref(),
        Some(format!("link:{normalized_linked_dir}").as_str()),
    );
    let LockfileResolution::Directory(dir) = &result.resolution else {
        panic!("expected directory resolution, got {:?}", result.resolution);
    };
    assert_eq!(dir.directory, normalized_linked_dir);
}

#[tokio::test]
async fn resolve_directory_specified_using_absolute_path_with_preserve_absolute_paths() {
    let (_tmp, project_dir) = fixture();
    let linked_dir = project_dir.join("..").lexical_normalize();
    let normalized_linked_dir = forward_slashes(linked_dir.display().to_string());

    let wd = WantedLocalDependency {
        bare_specifier: format!("link:{}", linked_dir.display()),
        injected: false,
    };
    let ctx = LocalResolverContext { preserve_absolute_paths: true };
    let result = resolve_from_local_scheme(&ctx, &wd, &opts(&project_dir))
        .await
        .expect("resolve")
        .expect("claims");

    assert_eq!(result.id.as_str(), format!("link:{normalized_linked_dir}"));
    assert_eq!(
        result.normalized_bare_specifier.as_deref(),
        Some(format!("link:{normalized_linked_dir}").as_str()),
    );
}

#[tokio::test]
async fn resolve_directory_specified_using_absolute_path_with_preserve_absolute_paths_and_file_scheme()
 {
    let (_tmp, project_dir) = fixture();
    let linked_dir = project_dir.join("..").lexical_normalize();
    let normalized_linked_dir = forward_slashes(linked_dir.display().to_string());

    let wd = WantedLocalDependency {
        bare_specifier: format!("file:{}", linked_dir.display()),
        injected: false,
    };
    let ctx = LocalResolverContext { preserve_absolute_paths: true };
    let result = resolve_from_local_scheme(&ctx, &wd, &opts(&project_dir))
        .await
        .expect("resolve")
        .expect("claims");

    assert_eq!(result.id.as_str(), format!("file:{normalized_linked_dir}"));
    assert_eq!(
        result.normalized_bare_specifier.as_deref(),
        Some(format!("file:{normalized_linked_dir}").as_str()),
    );
}

#[tokio::test]
async fn resolve_injected_directory() {
    let (_tmp, project_dir) = fixture();
    let wd = WantedLocalDependency { bare_specifier: "..".to_string(), injected: true };

    let result = resolve_from_local_path(&ctx_default(), &wd, &opts(&project_dir))
        .await
        .expect("resolve")
        .expect("claims");

    assert_eq!(result.id.as_str(), "file:..");
    assert_eq!(result.normalized_bare_specifier.as_deref(), Some("file:.."));
    let LockfileResolution::Directory(dir) = &result.resolution else {
        panic!("expected directory resolution, got {:?}", result.resolution);
    };
    assert_eq!(dir.directory, "..");
}

#[tokio::test]
async fn resolve_workspace_directory() {
    let (_tmp, project_dir) = fixture();
    let wd = WantedLocalDependency { bare_specifier: "workspace:..".to_string(), injected: false };

    let result = resolve_from_local_scheme(&ctx_default(), &wd, &opts(&project_dir))
        .await
        .expect("resolve")
        .expect("claims");

    assert_eq!(result.id.as_str(), "link:..");
    assert_eq!(result.normalized_bare_specifier.as_deref(), Some("link:.."));
}

#[tokio::test]
async fn resolve_directory_specified_using_the_file_protocol() {
    let (_tmp, project_dir) = fixture();
    let wd = WantedLocalDependency { bare_specifier: "file:..".to_string(), injected: false };

    let result = resolve_from_local_scheme(&ctx_default(), &wd, &opts(&project_dir))
        .await
        .expect("resolve")
        .expect("claims");

    assert_eq!(result.id.as_str(), "file:..");
    assert_eq!(result.normalized_bare_specifier.as_deref(), Some("file:.."));
    let LockfileResolution::Directory(dir) = &result.resolution else {
        panic!("expected directory resolution");
    };
    assert_eq!(dir.directory, "..");
}

#[tokio::test]
async fn resolve_directory_specified_using_the_link_protocol() {
    let (_tmp, project_dir) = fixture();
    let wd = WantedLocalDependency { bare_specifier: "link:..".to_string(), injected: false };

    let result = resolve_from_local_scheme(&ctx_default(), &wd, &opts(&project_dir))
        .await
        .expect("resolve")
        .expect("claims");

    assert_eq!(result.id.as_str(), "link:..");
    assert_eq!(result.normalized_bare_specifier.as_deref(), Some("link:.."));
}

/// Build a tarball for `pnpm-local-resolver@0.1.1` at `path` and return
/// its sha512 SSRI string.
fn write_tarball(path: &Path) -> String {
    let bytes = pnpm_testing_utils::fixtures::minimal_tarball("pnpm-local-resolver", "0.1.1");
    fs::write(path, &bytes).expect("write tarball");
    pnpm_testing_utils::fixtures::sha512_integrity(&bytes)
}

/// pnpm refuses a `file:` tarball whose bundled `package.json` names no
/// package with `ERR_PNPM_MISSING_PACKAGE_NAME`. Without the name the
/// dep path keys no lockfile entry, so resolving it would install a
/// dangling symlink off a lockfile that looks complete.
#[tokio::test]
async fn fail_when_a_tarball_manifest_names_no_package() {
    for manifest in [
        serde_json::json!({}),
        serde_json::json!({ "name": "" }),
        serde_json::json!({ "version": "1.0.0" }),
        serde_json::json!([1, 2]),
        serde_json::json!(null),
    ] {
        let tmp = TempDir::new().expect("tempdir");
        let test_dir = tmp.path().join("tgz");
        fs::create_dir_all(&test_dir).expect("create tgz dir");
        fs::write(
            test_dir.join("nameless-1.0.0.tgz"),
            pnpm_testing_utils::fixtures::tarball_with_manifest(&manifest),
        )
        .expect("write tarball");

        let wd = WantedLocalDependency {
            bare_specifier: "file:./nameless-1.0.0.tgz".to_string(),
            injected: false,
        };
        let err = resolve_from_local_scheme(&ctx_default(), &wd, &opts(&test_dir))
            .await
            .expect_err(&format!("a nameless manifest must be refused: {manifest}"));
        match err {
            ResolveLocalError::MissingPackageName { specifier } => {
                assert_eq!(specifier, "file:nameless-1.0.0.tgz");
            }
            other => panic!("expected MissingPackageName for {manifest}, got {other:?}"),
        }
    }
}

/// The bundled name becomes both a dep-path segment and a
/// `node_modules/<name>` directory. pnpm refuses an invalid one with
/// `ERR_PNPM_INVALID_DEPENDENCY_NAME`; letting `evil@name` through here
/// yields the dep path `evil@name@file:<path>`, which parses back out as
/// the unrelated package `evil` and links a dangling symlink.
#[tokio::test]
async fn fail_when_a_tarball_manifest_name_is_not_a_valid_npm_name() {
    for name in ["evil@name", " lead-space", "../escape", ".hidden", "node_modules"] {
        let tmp = TempDir::new().expect("tempdir");
        let test_dir = tmp.path().join("tgz");
        fs::create_dir_all(&test_dir).expect("create tgz dir");
        fs::write(
            test_dir.join("bad-1.0.0.tgz"),
            pnpm_testing_utils::fixtures::tarball_with_manifest(
                &serde_json::json!({ "name": name, "version": "1.0.0" }),
            ),
        )
        .expect("write tarball");

        let wd = WantedLocalDependency {
            bare_specifier: "file:./bad-1.0.0.tgz".to_string(),
            injected: false,
        };
        let err = resolve_from_local_scheme(&ctx_default(), &wd, &opts(&test_dir))
            .await
            .expect_err(&format!("an invalid package name must be refused: {name:?}"));
        match err {
            ResolveLocalError::InvalidPackageName { specifier, name: got } => {
                assert_eq!(specifier, "file:bad-1.0.0.tgz");
                assert_eq!(got, name);
            }
            other => panic!("expected InvalidPackageName for {name:?}, got {other:?}"),
        }
    }
}

/// An archive that ships no `package.json` at all is a different shape:
/// pnpm installs it, synthesizing a name from the alias, so resolution
/// must not refuse it here.
#[tokio::test]
async fn resolve_tarball_without_a_bundled_manifest() {
    let tmp = TempDir::new().expect("tempdir");
    let test_dir = tmp.path().join("tgz");
    fs::create_dir_all(&test_dir).expect("create tgz dir");
    fs::write(
        test_dir.join("no-manifest-1.0.0.tgz"),
        pnpm_testing_utils::fixtures::tarball_without_manifest(),
    )
    .expect("write tarball");

    let wd = WantedLocalDependency {
        bare_specifier: "file:./no-manifest-1.0.0.tgz".to_string(),
        injected: false,
    };
    let result = resolve_from_local_scheme(&ctx_default(), &wd, &opts(&test_dir))
        .await
        .expect("an archive without a manifest still resolves")
        .expect("claims");

    assert!(result.manifest.is_none(), "got {:?}", result.manifest);
}

#[tokio::test]
async fn resolve_file() {
    let tmp = TempDir::new().expect("tempdir");
    let test_dir = tmp.path().join("tgz");
    fs::create_dir_all(&test_dir).expect("create tgz dir");
    let tarball_path = test_dir.join("pnpm-local-resolver-0.1.1.tgz");
    let integrity = write_tarball(&tarball_path);

    let wd = WantedLocalDependency {
        bare_specifier: "./pnpm-local-resolver-0.1.1.tgz".to_string(),
        injected: false,
    };
    let result = resolve_from_local_path(&ctx_default(), &wd, &opts(&test_dir))
        .await
        .expect("resolve")
        .expect("claims");

    assert_eq!(result.id.as_str(), "file:pnpm-local-resolver-0.1.1.tgz");
    assert_eq!(
        result.normalized_bare_specifier.as_deref(),
        Some("file:pnpm-local-resolver-0.1.1.tgz"),
    );
    let LockfileResolution::Tarball(TarballResolution {
        tarball, integrity: got_integrity, ..
    }) = &result.resolution
    else {
        panic!("expected tarball resolution, got {:?}", result.resolution);
    };
    assert_eq!(tarball, "file:pnpm-local-resolver-0.1.1.tgz");
    assert_eq!(got_integrity.as_ref().expect("integrity").to_string(), integrity);
    assert_eq!(result.resolved_via, "local-filesystem");
    // The bundled manifest is what gives the dep path its `<name>@`
    // prefix, so a lockfile key can be parsed out of it.
    let manifest = result.manifest.as_deref().expect("bundled manifest");
    assert_eq!(
        manifest.get("name").and_then(serde_json::Value::as_str),
        Some("pnpm-local-resolver"),
    );
    assert_eq!(manifest.get("version").and_then(serde_json::Value::as_str), Some("0.1.1"));
}

#[tokio::test]
async fn resolve_file_when_lockfile_directory_differs_from_the_packages_dir() {
    let tmp = TempDir::new().expect("tempdir");
    let test_dir = tmp.path().join("tgz");
    fs::create_dir_all(&test_dir).expect("create tgz dir");
    let tarball_path = test_dir.join("pnpm-local-resolver-0.1.1.tgz");
    let _integrity = write_tarball(&tarball_path);

    let mut options = opts(&test_dir);
    options.lockfile_dir = Some(test_dir.join("..").lexical_normalize());

    let wd = WantedLocalDependency {
        bare_specifier: "./pnpm-local-resolver-0.1.1.tgz".to_string(),
        injected: false,
    };
    let result = resolve_from_local_path(&ctx_default(), &wd, &options)
        .await
        .expect("resolve")
        .expect("claims");

    assert_eq!(result.id.as_str(), "file:tgz/pnpm-local-resolver-0.1.1.tgz");
    assert_eq!(
        result.normalized_bare_specifier.as_deref(),
        Some("file:pnpm-local-resolver-0.1.1.tgz"),
    );
    let LockfileResolution::Tarball(TarballResolution { tarball, .. }) = &result.resolution else {
        panic!("expected tarball resolution");
    };
    assert_eq!(tarball, "file:tgz/pnpm-local-resolver-0.1.1.tgz");
}

#[tokio::test]
async fn resolve_tarball_specified_with_file_protocol() {
    let tmp = TempDir::new().expect("tempdir");
    let test_dir = tmp.path().join("tgz");
    fs::create_dir_all(&test_dir).expect("create tgz dir");
    let tarball_path = test_dir.join("pnpm-local-resolver-0.1.1.tgz");
    let _integrity = write_tarball(&tarball_path);

    let wd = WantedLocalDependency {
        bare_specifier: "file:./pnpm-local-resolver-0.1.1.tgz".to_string(),
        injected: false,
    };
    let result = resolve_from_local_scheme(&ctx_default(), &wd, &opts(&test_dir))
        .await
        .expect("resolve")
        .expect("claims");

    assert_eq!(result.id.as_str(), "file:pnpm-local-resolver-0.1.1.tgz");
    assert_eq!(
        result.normalized_bare_specifier.as_deref(),
        Some("file:pnpm-local-resolver-0.1.1.tgz"),
    );
}

#[tokio::test]
async fn resolve_file_with_different_integrity_force_fetch() {
    let tmp = TempDir::new().expect("tempdir");
    let test_dir = tmp.path().join("tgz");
    fs::create_dir_all(&test_dir).expect("create tgz dir");
    let tarball_path = test_dir.join("pnpm-local-resolver-0.1.1.tgz");
    let true_integrity = write_tarball(&tarball_path);

    let mut options = opts(&test_dir);
    options.current_pkg = Some(pnpm_resolving_local_resolver::LocalCurrentPkg {
        id: PkgResolutionId::from("file:pnpm-local-resolver-0.1.1.tgz"),
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: "file:pnpm-local-resolver-0.1.1.tgz".to_string(),
            integrity: Some(
                "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
                    .parse()
                    .expect("parse"),
            ),
            revision: None,
            git_hosted: None,
            path: None,
        }),
    });

    let wd = WantedLocalDependency {
        bare_specifier: "file:./pnpm-local-resolver-0.1.1.tgz".to_string(),
        injected: false,
    };
    let result = resolve_from_local_scheme(&ctx_default(), &wd, &options)
        .await
        .expect("resolve")
        .expect("claims");

    let LockfileResolution::Tarball(TarballResolution { integrity, .. }) = &result.resolution
    else {
        panic!("expected tarball resolution");
    };
    assert_eq!(integrity.as_ref().expect("integrity").to_string(), true_integrity);
}

#[tokio::test]
async fn fail_when_resolving_tarball_specified_with_the_link_protocol() {
    let tmp = TempDir::new().expect("tempdir");
    let test_dir = tmp.path().join("tgz");
    fs::create_dir_all(&test_dir).expect("create tgz dir");
    let tarball_path = test_dir.join("pnpm-local-resolver-0.1.1.tgz");
    let _ = write_tarball(&tarball_path);

    let wd = WantedLocalDependency {
        bare_specifier: "link:./pnpm-local-resolver-0.1.1.tgz".to_string(),
        injected: false,
    };
    let err = resolve_from_local_scheme(&ctx_default(), &wd, &opts(&test_dir))
        .await
        .expect_err("expected NOT_PACKAGE_DIRECTORY");
    assert!(matches!(err, ResolveLocalError::NotPackageDirectory { .. }), "got {err:?}");
}

#[tokio::test]
async fn fail_when_resolving_from_not_existing_directory_an_injected_dependency() {
    let tmp = TempDir::new().expect("tempdir");
    let project_dir = tmp.path();

    let wd = WantedLocalDependency {
        bare_specifier: "file:./dir-does-not-exist".to_string(),
        injected: false,
    };
    let err = resolve_from_local_scheme(&ctx_default(), &wd, &opts(project_dir))
        .await
        .expect_err("expected LINKED_PKG_DIR_NOT_FOUND");
    let expected = project_dir.join("dir-does-not-exist").display().to_string();
    match err {
        ResolveLocalError::LinkedPkgDirNotFound { path } => assert_eq!(path, expected),
        other => panic!("unexpected error: {other:?}"),
    }
}

/// A `file:./missing.tgz` spec funnels through the tarball branch
/// where `compute_tarball_integrity` raises ENOENT. The resolver must
/// surface the same `ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND` code the directory
/// branch raises for a missing `file:` target — both kinds of
/// missing `file:` target share one error path.
#[tokio::test]
async fn fail_when_resolving_missing_tarball_with_file_protocol() {
    let tmp = TempDir::new().expect("tempdir");
    let project_dir = tmp.path();

    let wd =
        WantedLocalDependency { bare_specifier: "file:./missing.tgz".to_string(), injected: false };
    let err = resolve_from_local_scheme(&ctx_default(), &wd, &opts(project_dir))
        .await
        .expect_err("expected LINKED_PKG_DIR_NOT_FOUND");
    let expected = project_dir.join("missing.tgz").display().to_string();
    {
        use miette::Diagnostic;
        let code = err.code().map(|c| c.to_string()).unwrap_or_default();
        assert_eq!(
            code, "ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND",
            "diagnostic code must match the upstream error contract",
        );
    }
    match err {
        ResolveLocalError::LinkedPkgDirNotFound { path } => assert_eq!(path, expected),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn do_not_fail_when_resolving_from_not_existing_directory() {
    let tmp = TempDir::new().expect("tempdir");
    let project_dir = tmp.path();

    let wd = WantedLocalDependency {
        bare_specifier: "link:./dir-does-not-exist".to_string(),
        injected: false,
    };
    let result = resolve_from_local_scheme(&ctx_default(), &wd, &opts(project_dir))
        .await
        .expect("resolve")
        .expect("claims");
    let manifest = result.manifest.as_ref().expect("manifest");
    assert_eq!(manifest.get("name").and_then(|value| value.as_str()), Some("dir-does-not-exist"));
    assert_eq!(manifest.get("version").and_then(|value| value.as_str()), Some("0.0.0"));
}

#[tokio::test]
async fn throw_error_when_the_path_protocol_is_used() {
    let tmp = TempDir::new().expect("tempdir");
    let project_dir = tmp.path();

    let wd = WantedLocalDependency { bare_specifier: "path:..".to_string(), injected: false };
    let err = resolve_from_local_scheme(&ctx_default(), &wd, &opts(project_dir))
        .await
        .expect_err("expected PATH_IS_UNSUPPORTED_PROTOCOL");
    match err {
        ResolveLocalError::Spec(
            pnpm_resolving_local_resolver::LocalSpecError::PathProtocolNotSupported(inner),
        ) => {
            assert_eq!(inner.bare_specifier, "path:..");
            assert_eq!(inner.protocol, "path:");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn resolve_from_local_path_ignores_explicit_local_schemes() {
    let tmp = TempDir::new().expect("tempdir");
    let project_dir = tmp.path();

    for bare in ["foo"] {
        let wd = WantedLocalDependency { bare_specifier: bare.to_string(), injected: false };
        let outcome = resolve_from_local_scheme(&ctx_default(), &wd, &opts(project_dir))
            .await
            .expect("resolve_from_local_scheme should not fail on bare specifier");
        assert!(outcome.is_none(), "scheme parser should defer on '{bare}'");
    }
    for bare in ["link:..", "workspace:..", "file:..", "path:.."] {
        let wd = WantedLocalDependency { bare_specifier: bare.to_string(), injected: false };
        let outcome = resolve_from_local_path(&ctx_default(), &wd, &opts(project_dir))
            .await
            .expect("resolve_from_local_path should not fail on scheme prefix");
        assert!(outcome.is_none(), "path parser should defer on '{bare}'");
    }
}

/// Lexically normalize `.` and `..` components without resolving
/// symlinks — matches Node's `path.resolve` semantics. `canonicalize`
/// would resolve macOS's `/var` → `/private/var` symlink and diverge
/// from the string-equality assertions.
trait LexicalNormalize: Sized {
    fn lexical_normalize(self) -> PathBuf;
}

impl LexicalNormalize for PathBuf {
    fn lexical_normalize(self) -> PathBuf {
        use std::path::Component;
        let mut out = PathBuf::new();
        for component in self.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    if !out.pop() {
                        out.push("..");
                    }
                }
                other => out.push(other.as_os_str()),
            }
        }
        out
    }
}

fn forward_slashes(input: String) -> String {
    if input.contains('\\') { input.replace('\\', "/") } else { input }
}
