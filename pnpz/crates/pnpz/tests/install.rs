use crate::support::{
    Remote,
    SYSTEM,
    Served,
};
use pnpz::{
    binary_cache::CacheError,
    install::{
        InstallError,
        install_closure,
    },
    lockfile::Lockfile,
    manifest::Manifest,
    project::{
        LockError,
        is_up_to_date,
        lock,
    },
    store::Store,
};
use pnpz_nix_store::StorePath;
use pretty_assertions::assert_eq;
use std::fs;

fn manifest(specs: &[&str]) -> Manifest {
    let mut manifest = Manifest::default();
    specs
        .iter()
        .for_each(|spec| manifest.add(spec).unwrap());
    manifest
}

fn systems() -> Vec<String> {
    vec![SYSTEM.to_string()]
}

async fn locked(remote: &Remote, manifest: &Manifest) -> Result<Lockfile, LockError> {
    let (catalog, cache) = remote.clients();
    lock(manifest, None, &systems(), (&catalog, &cache)).await
}

async fn installed(remote: &Remote, lockfile: &Lockfile) -> Result<Vec<String>, InstallError> {
    let (_, cache) = remote.clients();
    let fetched = std::sync::Mutex::new(Vec::new());
    install_closure(&cache, &remote.store(), &lockfile.closures[SYSTEM], |path, _| {
        fetched
            .lock()
            .unwrap()
            .push(path.name().to_string());
    })
    .await?;
    let mut fetched = fetched.into_inner().unwrap();
    fetched.sort();
    Ok(fetched)
}

fn tool_script(store: &Store, path: &StorePath) -> String {
    fs::read_to_string(
        store
            .location_of(path)
            .join("bin")
            .join(path.name()),
    )
    .unwrap()
}

/// The entries of the store directory, which include any staging leftovers.
fn store_entries(store: &Store) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(store.root().join("store"))
        .unwrap()
        .map(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn locks_and_installs_a_package_with_its_closure() {
    let mut remote = Remote::start().await;
    let (libc, _) = remote.add_path("libc", "libc", &[], Served::Xz).await;
    let (shared, _) = remote.add_path("shared", "shared", &[&libc], Served::Uncompressed).await;
    let self_reference = crate::support::store_path("tool-1.2.3");
    let (tool, _) =
        remote.add_path("tool-1.2.3", "ran", &[&shared, &libc, &self_reference], Served::Zstd)
            .await;
    remote.add_package("tool", "1.2.3", &tool).await;

    let manifest = manifest(&["tool@1.2"]);
    let lockfile = locked(&remote, &manifest).await.unwrap();

    let package = &lockfile.packages["tool"];
    assert_eq!((package.specifier.as_str(), package.version.as_str()), ("1.2", "1.2.3"));
    assert_eq!(package.systems[SYSTEM].outputs.values().collect::<Vec<_>>(), [&tool]);
    let mut closure: Vec<&StorePath> = lockfile.closures[SYSTEM].keys().collect();
    closure.sort_by_key(|path| path.name());
    assert_eq!(closure, [&libc, &shared, &tool]);
    assert!(is_up_to_date(&manifest, &lockfile, &systems()));

    assert_eq!(installed(&remote, &lockfile).await.unwrap(), ["libc", "shared", "tool-1.2.3"]);
    let store = remote.store();
    assert_eq!(tool_script(&store, &tool), "#!/bin/sh\necho ran\n");
    assert_eq!(tool_script(&store, &libc), "#!/bin/sh\necho libc\n");
    assert_eq!(store_entries(&store).len(), 3);
}

#[tokio::test]
async fn a_second_install_downloads_nothing() {
    let mut remote = Remote::start().await;
    let (tool, archive) = remote.add_path("tool", "ran", &[], Served::Zstd).await;
    remote.add_package("tool", "1.0.0", &tool).await;
    let lockfile = locked(&remote, &manifest(&["tool"])).await.unwrap();

    assert_eq!(installed(&remote, &lockfile).await.unwrap(), ["tool"]);
    assert_eq!(installed(&remote, &lockfile).await.unwrap(), Vec::<String>::new());
    archive.expect(1).assert_async().await;
}

#[tokio::test]
async fn an_unchanged_manifest_keeps_its_locked_versions() {
    let mut remote = Remote::start().await;
    let (tool, _) = remote.add_path("tool", "ran", &[], Served::Zstd).await;
    let catalog_entry = remote.add_package("tool", "1.0.0", &tool).await;
    let manifest = manifest(&["tool"]);
    let first = locked(&remote, &manifest).await.unwrap();

    let (catalog, cache) = remote.clients();
    let second = lock(&manifest, Some(&first), &systems(), (&catalog, &cache))
        .await
        .unwrap();

    assert_eq!(second, first);
    catalog_entry.expect(1).assert_async().await;
}

#[tokio::test]
async fn a_changed_manifest_is_not_up_to_date() {
    let mut remote = Remote::start().await;
    let (tool, _) = remote.add_path("tool", "ran", &[], Served::Zstd).await;
    remote.add_package("tool", "1.0.0", &tool).await;
    let lockfile = locked(&remote, &manifest(&["tool"])).await.unwrap();

    assert!(!is_up_to_date(&manifest(&["tool@2"]), &lockfile, &systems()));
    assert!(!is_up_to_date(&manifest(&["tool", "other"]), &lockfile, &systems()));
    assert!(!is_up_to_date(&manifest(&[]), &lockfile, &systems()));
    assert!(!is_up_to_date(&manifest(&["tool"]), &lockfile, &["aarch64-darwin".to_string()]));
}

#[tokio::test]
async fn rejects_an_archive_that_differs_from_its_signed_hash() {
    let mut remote = Remote::start().await;
    let (tool, _) = remote.add_path("tool", "ran", &[], Served::Tampered).await;
    remote.add_package("tool", "1.0.0", &tool).await;
    let lockfile = locked(&remote, &manifest(&["tool"])).await.unwrap();

    let error = installed(&remote, &lockfile).await.unwrap_err();

    assert!(matches!(error, InstallError::Cache(CacheError::HashMismatch { .. })), "{error}");
    assert_eq!(store_entries(&remote.store()), Vec::<String>::new());
}

#[tokio::test]
async fn rejects_a_path_signed_by_an_untrusted_key() {
    let mut remote = Remote::start().await;
    let (tool, _) = remote.add_path("tool", "ran", &[], Served::UntrustedSignature).await;
    remote.add_package("tool", "1.0.0", &tool).await;

    let error = locked(&remote, &manifest(&["tool"])).await.unwrap_err();

    assert!(matches!(error, LockError::Cache(CacheError::Untrusted(_))), "{error}");
    assert!(error.to_string().contains("someone-else-1"), "{error}");
}

#[tokio::test]
async fn rejects_a_cache_whose_hash_differs_from_the_lockfile() {
    let mut remote = Remote::start().await;
    let (tool, _) = remote.add_path("tool", "ran", &[], Served::Zstd).await;
    remote.add_package("tool", "1.0.0", &tool).await;
    let lockfile = locked(&remote, &manifest(&["tool"])).await.unwrap();

    // The cache now serves a validly signed, different build of the path.
    let mut republished = Remote::start().await;
    republished.add_path("tool", "rebuilt", &[], Served::Zstd).await;

    let error = installed(&republished, &lockfile).await.unwrap_err();

    assert!(matches!(error, InstallError::LockedHashMismatch { .. }), "{error}");
    assert_eq!(store_entries(&republished.store()), Vec::<String>::new());
}

#[tokio::test]
async fn reports_a_package_the_catalog_lacks_and_a_path_the_cache_lacks() {
    let mut remote = Remote::start().await;
    remote.server
        .mock("GET", mockito::Matcher::Any)
        .with_status(404)
        .create_async()
        .await;
    let error = locked(&remote, &manifest(&["missing"])).await.unwrap_err();
    assert_eq!(error.to_string(), "the catalog has no package missing@latest");

    let mut remote = Remote::start().await;
    remote.add_package("tool", "1.0.0", &crate::support::store_path("tool")).await;
    let narinfo = mockito::Matcher::Regex(r"\.narinfo$".to_string());
    remote.server
        .mock("GET", narinfo)
        .with_status(404)
        .create_async()
        .await;
    let error = locked(&remote, &manifest(&["tool"])).await.unwrap_err();
    assert!(matches!(error, LockError::Cache(CacheError::NotFound { .. })), "{error}");
}

#[tokio::test]
async fn reports_a_package_with_no_build_for_the_system() {
    let mut remote = Remote::start().await;
    let (tool, _) = remote.add_path("tool", "ran", &[], Served::Zstd).await;
    remote.add_package("tool", "1.0.0", &tool).await;
    let (catalog, cache) = remote.clients();

    let systems = ["riscv64-linux".to_string()];
    let error = lock(&manifest(&["tool"]), None, &systems, (&catalog, &cache))
        .await
        .unwrap_err();

    assert_eq!(error.to_string(), "tool@1.0.0 has no build for riscv64-linux");
}
