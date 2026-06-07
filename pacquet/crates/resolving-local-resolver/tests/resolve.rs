//! Port of pnpm's
//! [`resolving/local-resolver/test/index.ts`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/test/index.ts).
//!
//! Each `#[tokio::test]` mirrors one upstream `test(...)` block; the
//! upstream test name is preserved in the Rust function name.

use pacquet_lockfile::{LockfileResolution, TarballResolution};
use pacquet_resolving_local_resolver::{
    LocalResolverContext, LocalResolverOptions, LocalResolverUpdate, ResolveLocalError,
    WantedLocalDependency, resolve_from_local_path, resolve_from_local_scheme,
};
use pacquet_resolving_resolver_base::PkgResolutionId;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

/// Set up a `<tmp>/inner/` directory with a package.json carrying the
/// `name` upstream's tests assert against. Returns `(tmp, inner)` so
/// the temp dir lives as long as the test.
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

/// Build a tiny tarball at `path` and return its sha512 SSRI string.
fn write_tarball(path: &Path) -> String {
    // Any bytes work — the test asserts the integrity round-trips
    // through the resolver, not a specific upstream-pinned value.
    let bytes: &[u8] = b"\x1f\x8b\x08\x00fake-tarball-bytes-for-test\n";
    fs::write(path, bytes).expect("write tarball");
    let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    opts.input(bytes);
    opts.result().to_string()
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
    options.current_pkg = Some(pacquet_resolving_local_resolver::LocalCurrentPkg {
        id: PkgResolutionId::from("file:pnpm-local-resolver-0.1.1.tgz"),
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: "file:pnpm-local-resolver-0.1.1.tgz".to_string(),
            integrity: Some(
                "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
                    .parse()
                    .expect("parse"),
            ),
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
/// surface the same `LINKED_PKG_DIR_NOT_FOUND` code the directory
/// branch raises for a missing `file:` target — both kinds of
/// missing `file:` target share one pnpm-compatible error path
/// (`resolveSpec` upstream:
/// <https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/index.ts#L108-L141>).
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
            pacquet_resolving_local_resolver::LocalSpecError::PathProtocolNotSupported(inner),
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
/// symlinks — matches Node's `path.resolve` semantics that the
/// upstream tests compare against. `canonicalize` would resolve
/// macOS's `/var` → `/private/var` symlink and diverge from the
/// upstream string-equality assertions.
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
