use crate::Lockfile;
use pretty_assertions::assert_eq;
use tempfile::tempdir;
use text_block_macros::text_block;

/// Single-document lockfile body shared across the loader tests below.
const MAIN_DOC: &str = text_block! {
    "lockfileVersion: '9.0'"
    ""
    "settings:"
    "  autoInstallPeers: true"
    "  excludeLinksFromLockfile: false"
    ""
    "importers:"
    ""
    "  .:"
    "    dependencies:"
    "      react:"
    "        specifier: ^17.0.2"
    "        version: 17.0.2"
    ""
    "packages:"
    ""
    "  react@17.0.2:"
    "    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}"
    ""
    "snapshots:"
    ""
    "  react@17.0.2: {}"
};

/// Env-document prelude pnpm v11 writes when `packageManager` /
/// `devEngines.runtime` triggers a package-manager-bootstrap entry.
/// Shape matches upstream's `EnvLockfile` at
/// <https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/types/src/index.ts#L187-L194>.
const ENV_DOC: &str = text_block! {
    "lockfileVersion: '9.0'"
    ""
    "importers:"
    ""
    "  .:"
    "    configDependencies: {}"
    "    packageManagerDependencies:"
    "      pnpm:"
    "        specifier: ^11.0.0"
    "        version: 11.0.8"
    ""
    "packages:"
    ""
    "  pnpm@11.0.8:"
    "    resolution: {integrity: sha512-TECX4d0tQjcsTn+lp5H/KPx1pITHrBkuZLHfD97xdZS6mC+bT+2a37PHV4RvVlt5mydj+zcz0d4by4LPRmhJEg==}"
    "    hasBin: true"
    ""
    "snapshots:"
    ""
    "  pnpm@11.0.8: {}"
};

fn write_lockfile(content: &str) -> tempfile::TempDir {
    let tmp = tempdir().expect("create tempdir");
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");
    std::fs::create_dir_all(&virtual_store_dir).expect("mkdir virtual_store_dir");
    std::fs::write(virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME), content)
        .expect("write lock.yaml");
    tmp
}

/// A pnpm v11 multi-document lockfile (env document + main document)
/// must parse as the *second* document. The env document carries the
/// package-manager bootstrap and is intentionally ignored by the
/// install path. Mirrors upstream's
/// [`_read`](https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/fs/src/read.ts#L103-L110)
/// which feeds the lockfile content through `extractMainDocument`
/// before parsing.
#[test]
fn parses_main_document_from_combined_yaml() {
    let combined = format!("---\n{ENV_DOC}\n---\n{MAIN_DOC}");
    let tmp = write_lockfile(&combined);
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");

    let combined_loaded = Lockfile::load_current_from_virtual_store_dir(&virtual_store_dir)
        .expect("load combined lockfile")
        .expect("combined lockfile should be present");

    // Same content parsed without the env prelude must produce an
    // identical `Lockfile` — proves the env document was skipped and
    // didn't bleed into the parsed structure.
    let tmp_main = write_lockfile(MAIN_DOC);
    let main_only_dir = tmp_main.path().join("node_modules").join(".pacquet");
    let main_only_loaded = Lockfile::load_current_from_virtual_store_dir(&main_only_dir)
        .expect("load main-only lockfile")
        .expect("main-only lockfile should be present");

    assert_eq!(combined_loaded, main_only_loaded);
}

/// An env-only lockfile (file starts with `---\n` but has no second
/// document) reads back as `Ok(None)`, mirroring upstream's empty-main-
/// document short-circuit at
/// <https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/fs/src/read.ts#L105-L110>.
#[test]
fn env_only_lockfile_loads_as_none() {
    let env_only = format!("---\n{ENV_DOC}\n");
    let tmp = write_lockfile(&env_only);
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");

    let result = Lockfile::load_current_from_virtual_store_dir(&virtual_store_dir)
        .expect("env-only lockfile should not error");
    assert!(result.is_none(), "expected None for env-only lockfile, got: {result:?}");
}
