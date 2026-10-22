use super::{
    Delegation, IncludeError, IncludeSource, SettingOwner, SettingSlot, WorkspaceManifestDocument,
    read_workspace_manifest_document,
};
use pretty_assertions::assert_eq;
use std::{fs, path::Path};
use tempfile::TempDir;

fn workspace(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("create temp dir");
    for (path, text) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().expect("file has a parent")).expect("create parent");
        fs::write(path, text).expect("write file");
    }
    dir
}

fn load(dir: &Path) -> Result<WorkspaceManifestDocument, IncludeError> {
    read_workspace_manifest_document(dir).map(|document| document.expect("root manifest exists"))
}

fn merged(dir: &Path) -> yaml_serde::Value {
    let document = load(dir).expect("load workspace");
    yaml_serde::from_str(&document.text).expect("merged text parses")
}

fn setting(name: &str) -> SettingSlot {
    SettingSlot::Setting(name.to_string())
}

fn local_owner(document: &WorkspaceManifestDocument, slot: &SettingSlot) -> Option<String> {
    match document.includes
        .as_ref()
        .expect("has includes")
        .owner(slot)
    {
        SettingOwner::Include(IncludeSource::Local { spec, .. }) => Some(spec.clone()),
        SettingOwner::Include(IncludeSource::Package(name)) => Some(name.clone()),
        SettingOwner::Root => Some("root".to_string()),
        SettingOwner::Unowned => None,
    }
}

#[test]
fn manifest_without_include_is_returned_as_written() {
    let text = "packages:\n  - 'pkgs/*'\n# a comment\n";
    let dir = workspace(&[("pnpm-workspace.yaml", text)]);
    let document = load(dir.path()).expect("load");
    assert_eq!(document.text, text);
    assert!(document.includes.is_none());
}

#[test]
fn missing_root_manifest_is_none() {
    let dir = tempfile::tempdir().expect("create temp dir");
    assert!(read_workspace_manifest_document(dir.path()).expect("read").is_none());
}

#[test]
fn local_sources_are_merged_into_the_root_settings() {
    let dir = workspace(&[
        (
            "pnpm-workspace.yaml",
            "include:\n  ./config/security.yaml: '*'\n  ./config/catalog.yml: [catalog]\n  ./config/workspace.yaml: [packages]\nnodeLinker: hoisted\n",
        ),
        ("config/security.yaml", "minimumReleaseAge: 1440\n"),
        ("config/catalog.yml", "catalog:\n  react: ^19.0.0\n"),
        ("config/workspace.yaml", "packages: ['../pkgs/*']\n"),
    ]);
    let expected: yaml_serde::Value = yaml_serde::from_str(
        "nodeLinker: hoisted\nminimumReleaseAge: 1440\ncatalog:\n  react: ^19.0.0\npackages: ['pkgs/*']\n",
    )
    .expect("parse expected");
    assert_eq!(merged(dir.path()), expected);
}

#[test]
fn unreferenced_files_are_not_loaded() {
    let dir = workspace(&[
        ("pnpm-workspace.yaml", "include:\n  ./config/catalog.yaml: [catalog]\n"),
        ("config/catalog.yaml", ""),
        ("config/security.yaml", "minimumReleaseAge: 0\n"),
    ]);
    let expected: yaml_serde::Value = yaml_serde::from_str("{}").expect("parse expected");
    assert_eq!(merged(dir.path()), expected);
}

#[test]
fn named_catalogs_from_several_sources_are_merged() {
    let dir = workspace(&[
        (
            "pnpm-workspace.yaml",
            "include:\n  ./catalog.yaml: [catalog]\n  ./legacy.yaml: [catalogs.react18]\ncatalogs:\n  tools:\n    tsx: ^4.0.0\n",
        ),
        ("catalog.yaml", "catalog:\n  react: ^19.0.0\n"),
        ("legacy.yaml", "catalogs:\n  react18:\n    react: ^18.3.1\n"),
    ]);
    let expected: yaml_serde::Value = yaml_serde::from_str(
        "catalogs:\n  tools:\n    tsx: ^4.0.0\n  react18:\n    react: ^18.3.1\ncatalog:\n  react: ^19.0.0\n",
    )
    .expect("parse expected");
    assert_eq!(merged(dir.path()), expected);
}

#[test]
fn listed_settings_are_owned_even_when_omitted() {
    let dir = workspace(&[
        (
            "pnpm-workspace.yaml",
            "include:\n  ./deploy.yaml: [nodeLinker, injectWorkspacePackages]\n",
        ),
        ("deploy.yaml", ""),
    ]);
    let document = load(dir.path()).expect("load");
    assert_eq!(local_owner(&document, &setting("nodeLinker")).as_deref(), Some("./deploy.yaml"));
    assert_eq!(local_owner(&document, &setting("minimumReleaseAge")), None);
}

#[test]
fn unrestricted_sources_own_what_they_define() {
    let dir = workspace(&[
        ("pnpm-workspace.yaml", "include:\n  ./security.yaml: '*'\nnodeLinker: hoisted\n"),
        ("security.yaml", "minimumReleaseAge: null\n"),
    ]);
    let document = load(dir.path()).expect("load");
    assert_eq!(
        local_owner(&document, &setting("minimumReleaseAge")).as_deref(),
        Some("./security.yaml"),
    );
    assert_eq!(local_owner(&document, &setting("nodeLinker")).as_deref(), Some("root"));
    assert_eq!(local_owner(&document, &setting("trustPolicy")), None);
}

#[test]
fn whole_catalogs_delegation_owns_new_catalogs() {
    let dir = workspace(&[
        ("pnpm-workspace.yaml", "include:\n  ./catalogs.yaml: [catalogs]\n"),
        ("catalogs.yaml", "catalog:\n  a: 1.0.0\n"),
    ]);
    let document = load(dir.path()).expect("load");
    let owner = local_owner(&document, &SettingSlot::Catalog("brand-new".to_string()));
    assert_eq!(owner.as_deref(), Some("./catalogs.yaml"));
}

#[test]
fn aliases_share_an_owner() {
    let dir = workspace(&[
        (
            "pnpm-workspace.yaml",
            "include:\n  ./store.yaml: [virtualStoreType]\nenableGlobalVirtualStore: true\n",
        ),
        ("store.yaml", ""),
    ]);
    let error = load(dir.path()).expect_err("alias defined outside its owner");
    assert!(matches!(error, IncludeError::DuplicateSetting { .. }), "{error:?}");
}

#[test]
fn package_sources_are_listed_with_their_settings() {
    let dir = workspace(&[(
        "pnpm-workspace.yaml",
        "configDependencies:\n  '@acme/pnpm-policy': 3.1.0\ninclude:\n  '@acme/pnpm-policy': [minimumReleaseAge, trustPolicy]\n",
    )]);
    let document = load(dir.path()).expect("load");
    let includes = document.includes.expect("has includes");
    let packages: Vec<_> = includes.packages().collect();
    assert_eq!(
        packages,
        [("@acme/pnpm-policy", [setting("minimumReleaseAge"), setting("trustPolicy")].as_slice())],
    );
    assert_eq!(
        includes.includes[0].delegation,
        Delegation::Listed(vec![setting("minimumReleaseAge"), setting("trustPolicy")]),
    );
}

#[test]
fn config_dependencies_may_come_from_a_local_source() {
    let dir = workspace(&[
        ("pnpm-workspace.yaml", "include:\n  ./trusted.yaml: '*'\n  policy: [minimumReleaseAge]\n"),
        ("trusted.yaml", "configDependencies:\n  policy: 1.0.0\n"),
    ]);
    load(dir.path()).expect("load");
}

fn load_error(files: &[(&str, &str)]) -> IncludeError {
    let dir = workspace(files);
    let error = load(dir.path()).expect_err("load must fail");
    eprintln!("{error}");
    error
}

#[test]
fn definition_outside_the_list() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", "include:\n  ./catalog.yaml: [catalog]\n"),
        ("catalog.yaml", "minimumReleaseAgeExclude: ['*']\n"),
    ]);
    assert!(matches!(error, IncludeError::SettingNotDelegated { .. }), "{error:?}");
}

#[test]
fn named_catalog_outside_the_list() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", "include:\n  ./legacy.yaml: [catalogs.react18]\n"),
        ("legacy.yaml", "catalogs:\n  react17: {}\n"),
    ]);
    assert!(matches!(error, IncludeError::SettingNotDelegated { .. }), "{error:?}");
}

#[test]
fn root_definition_of_a_delegated_setting() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", "include:\n  ./deploy.yaml: [nodeLinker]\nnodeLinker: hoisted\n"),
        ("deploy.yaml", ""),
    ]);
    assert!(matches!(error, IncludeError::DuplicateSetting { .. }), "{error:?}");
}

#[test]
fn unrestricted_definition_of_a_package_setting() {
    let error = load_error(&[
        (
            "pnpm-workspace.yaml",
            "configDependencies:\n  policy: 1.0.0\ninclude:\n  policy: [minimumReleaseAge]\n  ./security.yaml: '*'\n",
        ),
        ("security.yaml", "minimumReleaseAge: 0\n"),
    ]);
    assert!(matches!(error, IncludeError::DuplicateSetting { .. }), "{error:?}");
}

#[test]
fn overlapping_lists() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", "include:\n  ./a.yaml: [nodeLinker]\n  ./b.yaml: [nodeLinker]\n"),
        ("a.yaml", ""),
        ("b.yaml", ""),
    ]);
    assert!(matches!(error, IncludeError::DuplicateSetting { .. }), "{error:?}");
}

#[test]
fn whole_catalogs_overlapping_a_named_catalog() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", "include:\n  ./a.yaml: [catalogs]\n  ./b.yaml: [catalogs.foo]\n"),
        ("a.yaml", ""),
        ("b.yaml", ""),
    ]);
    assert!(matches!(error, IncludeError::DuplicateSetting { .. }), "{error:?}");
}

#[test]
fn catalog_defined_in_two_unrestricted_sources() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", "include:\n  ./a.yaml: '*'\ncatalogs:\n  foo:\n    a: 1.0.0\n"),
        ("a.yaml", "catalogs:\n  foo:\n    b: 1.0.0\n"),
    ]);
    assert!(matches!(error, IncludeError::DuplicateSetting { .. }), "{error:?}");
}

#[test]
fn default_catalog_under_both_spellings() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", "include:\n  ./a.yaml: '*'\ncatalog:\n  a: 1.0.0\n"),
        ("a.yaml", "catalogs:\n  default:\n    b: 1.0.0\n"),
    ]);
    assert!(matches!(error, IncludeError::DuplicateSetting { .. }), "{error:?}");
}

#[test]
fn repeated_selector() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", "include:\n  ./a.yaml: [catalog, catalogs.default]\n"),
        ("a.yaml", ""),
    ]);
    assert!(matches!(error, IncludeError::RepeatedSelector { .. }), "{error:?}");
}

#[test]
fn nested_selector() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", "include:\n  ./a.yaml: [auditConfig.ignoreGhsas]\n"),
        ("a.yaml", ""),
    ]);
    assert!(matches!(error, IncludeError::InvalidSelector { .. }), "{error:?}");
}

#[test]
fn missing_local_source() {
    let error = load_error(&[("pnpm-workspace.yaml", "include:\n  ./missing.yaml: '*'\n")]);
    assert!(matches!(error, IncludeError::ReadFragment { .. }), "{error:?}");
}

#[test]
fn directory_source() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", "include:\n  ./dir.yaml: '*'\n"),
        ("dir.yaml/file", ""),
    ]);
    assert!(matches!(error, IncludeError::NotAFile { .. }), "{error:?}");
}

#[test]
fn invalid_yaml_source() {
    let error =
        load_error(&[("pnpm-workspace.yaml", "include:\n  ./a.yaml: '*'\n"), ("a.yaml", "a: [\n")]);
    assert!(matches!(error, IncludeError::ParseFragment { .. }), "{error:?}");
}

#[test]
fn duplicate_key_in_source() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", "include:\n  ./a.yaml: '*'\n"),
        ("a.yaml", "a: 1\na: 2\n"),
    ]);
    assert!(matches!(error, IncludeError::ParseFragment { .. }), "{error:?}");
}

/// The settings readers reject this root manifest themselves, so its include
/// index cannot be ignored by one reader and honored by another.
#[test]
fn root_with_a_duplicate_key_is_left_to_the_settings_readers() {
    let text = "include:\n  ./a.yaml: '*'\nnodeLinker: a\nnodeLinker: b\n";
    let dir = workspace(&[("pnpm-workspace.yaml", text), ("a.yaml", "")]);
    let document = load(dir.path()).expect("load");
    assert_eq!(document.text, text);
    assert!(document.includes.is_none());
    serde_saphyr::from_str::<indexmap::IndexMap<String, serde::de::IgnoredAny>>(text)
        .expect_err("the settings readers reject a duplicate key");
}

#[test]
fn nested_include() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", "include:\n  ./a.yaml: '*'\n"),
        ("a.yaml", "include: {}\n"),
    ]);
    assert!(matches!(error, IncludeError::NestedInclude { .. }), "{error:?}");
}

#[test]
fn root_included_from_itself() {
    let error = load_error(&[("pnpm-workspace.yaml", "include:\n  ./pnpm-workspace.yaml: '*'\n")]);
    assert!(matches!(error, IncludeError::IncludesRoot { .. }), "{error:?}");
}

#[test]
fn same_file_under_two_specs() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", "include:\n  ./a.yaml: '*'\n  ./config/../a.yaml: [catalog]\n"),
        ("a.yaml", ""),
        ("config/.keep", ""),
    ]);
    assert!(matches!(error, IncludeError::DuplicateSource { .. }), "{error:?}");
}

#[test]
fn glob_source() {
    let error = load_error(&[("pnpm-workspace.yaml", "include:\n  ./config/*.yaml: '*'\n")]);
    assert!(matches!(error, IncludeError::InvalidLocalPath { .. }), "{error:?}");
}

#[test]
fn non_yaml_source() {
    let error = load_error(&[("pnpm-workspace.yaml", "include:\n  ./config.json: '*'\n")]);
    assert!(matches!(error, IncludeError::InvalidLocalPath { .. }), "{error:?}");
}

#[test]
fn absolute_source() {
    let error = load_error(&[("pnpm-workspace.yaml", "include:\n  /etc/config.yaml: '*'\n")]);
    assert!(matches!(error, IncludeError::InvalidSource { .. }), "{error:?}");
}

#[test]
fn unrestricted_package() {
    let error = load_error(&[(
        "pnpm-workspace.yaml",
        "configDependencies:\n  policy: 1.0.0\ninclude:\n  policy: '*'\n",
    )]);
    assert!(matches!(error, IncludeError::UnrestrictedPackage { .. }), "{error:?}");
}

#[test]
fn undeclared_package() {
    let error = load_error(&[("pnpm-workspace.yaml", "include:\n  policy: [minimumReleaseAge]\n")]);
    assert!(matches!(error, IncludeError::UndeclaredConfigDependency { .. }), "{error:?}");
}

#[test]
fn forbidden_package_setting() {
    let error = load_error(&[(
        "pnpm-workspace.yaml",
        "configDependencies:\n  policy: 1.0.0\ninclude:\n  policy: [pnpmfile]\n",
    )]);
    assert!(matches!(error, IncludeError::ForbiddenPackageSetting { .. }), "{error:?}");
}

#[test]
fn include_index_not_a_mapping() {
    let error = load_error(&[("pnpm-workspace.yaml", "include: ['./a.yaml']\n")]);
    assert!(matches!(error, IncludeError::IndexNotAMapping), "{error:?}");
}

#[test]
fn include_order_does_not_change_the_result() {
    let forward = workspace(&[
        ("pnpm-workspace.yaml", "include:\n  ./a.yaml: [nodeLinker]\n  ./b.yaml: '*'\n"),
        ("a.yaml", "nodeLinker: hoisted\n"),
        ("b.yaml", "minimumReleaseAge: 10\n"),
    ]);
    let backward = workspace(&[
        ("pnpm-workspace.yaml", "include:\n  ./b.yaml: '*'\n  ./a.yaml: [nodeLinker]\n"),
        ("a.yaml", "nodeLinker: hoisted\n"),
        ("b.yaml", "minimumReleaseAge: 10\n"),
    ]);
    assert_eq!(merged(forward.path()), merged(backward.path()));
}

#[test]
fn placeholders_survive_the_merge_unquoted() {
    let dir = workspace(&[
        ("pnpm-workspace.yaml", "include:\n  ./a.yaml: '*'\n"),
        ("a.yaml", "minimumReleaseAge: ${AGE}\n"),
    ]);
    let document = load(dir.path()).expect("load");
    assert_eq!(document.text, "minimumReleaseAge: ${AGE}\n");
}

const POLICY_ROOT: &str = "configDependencies:\n  '@acme/policy': 3.1.0\ninclude:\n  '@acme/policy': [minimumReleaseAge, trustPolicy, catalogs.shared]\n";
const POLICY_DIR: &str = "node_modules/.pnpm-config/@acme/policy";

fn policy_workspace(manifest: &str, files: &[(&str, &str)]) -> TempDir {
    let manifest_path = format!("{POLICY_DIR}/package.json");
    let mut all = vec![("pnpm-workspace.yaml", POLICY_ROOT), (manifest_path.as_str(), manifest)];
    let owned: Vec<(String, &str)> = files
        .iter()
        .map(|(path, text)| (format!("{POLICY_DIR}/{path}"), *text))
        .collect();
    all.extend(
        owned
            .iter()
            .map(|(path, text)| (path.as_str(), *text)),
    );
    workspace(&all)
}

#[test]
fn an_installed_package_entry_is_merged() {
    let dir = policy_workspace(
        r#"{"name":"@acme/policy","pnpm":{"config":"./config.yaml"}}"#,
        &[("config.yaml", "minimumReleaseAge: 1440\ncatalogs:\n  shared:\n    react: ^19.0.0\n")],
    );
    let document = load(dir.path()).expect("load");
    let includes = document.includes.as_ref().expect("has includes");
    let unavailable: Vec<&str> = includes.unavailable_packages().collect();
    assert_eq!(unavailable, Vec::<&str>::new());
    let expected: yaml_serde::Value = yaml_serde::from_str(
        "configDependencies:\n  '@acme/policy': 3.1.0\nminimumReleaseAge: 1440\ncatalogs:\n  shared:\n    react: ^19.0.0\n",
    )
    .expect("parse expected");
    assert_eq!(merged(dir.path()), expected);
}

#[test]
fn a_package_not_installed_is_unavailable_but_keeps_its_settings() {
    let dir = workspace(&[("pnpm-workspace.yaml", POLICY_ROOT)]);
    let document = load(dir.path()).expect("load");
    let includes = document.includes.as_ref().expect("has includes");
    assert_eq!(includes.unavailable_packages().collect::<Vec<_>>(), ["@acme/policy"]);
    assert_eq!(local_owner(&document, &setting("trustPolicy")).as_deref(), Some("@acme/policy"));
}

#[test]
fn package_entry_with_an_undelegated_setting() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", POLICY_ROOT),
        (
            "node_modules/.pnpm-config/@acme/policy/package.json",
            r#"{"pnpm":{"config":"config.yml"}}"#,
        ),
        ("node_modules/.pnpm-config/@acme/policy/config.yml", "minimumReleaseAgeExclude: ['*']\n"),
    ]);
    assert!(matches!(error, IncludeError::SettingNotDelegated { .. }), "{error:?}");
}

#[test]
fn package_without_an_entry() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", POLICY_ROOT),
        ("node_modules/.pnpm-config/@acme/policy/package.json", r#"{"name":"@acme/policy"}"#),
    ]);
    assert!(matches!(error, IncludeError::MissingPackageEntry { .. }), "{error:?}");
}

#[test]
fn package_entry_escaping_the_package() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", POLICY_ROOT),
        (
            "node_modules/.pnpm-config/@acme/policy/package.json",
            r#"{"pnpm":{"config":"../../outside.yaml"}}"#,
        ),
        ("node_modules/.pnpm-config/outside.yaml", "minimumReleaseAge: 0\n"),
    ]);
    assert!(matches!(error, IncludeError::InvalidPackageEntry { .. }), "{error:?}");
}

#[test]
fn package_entry_that_is_not_yaml() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", POLICY_ROOT),
        (
            "node_modules/.pnpm-config/@acme/policy/package.json",
            r#"{"pnpm":{"config":"./pnpmfile.mjs"}}"#,
        ),
    ]);
    assert!(matches!(error, IncludeError::InvalidPackageEntry { .. }), "{error:?}");
}

#[cfg(unix)]
#[test]
fn package_entry_symlinked_outside_the_package_is_rejected() {
    let dir = policy_workspace(r#"{"pnpm":{"config":"./config.yaml"}}"#, &[]);
    fs::write(dir.path().join("outside.yaml"), "minimumReleaseAge: 0\n").expect("write outside");
    std::os::unix::fs::symlink(
        dir.path().join("outside.yaml"),
        dir.path()
            .join(POLICY_DIR)
            .join("config.yaml"),
    )
    .expect("symlink");
    let error = load(dir.path()).expect_err("escaping symlink");
    assert!(matches!(error, IncludeError::InvalidPackageEntry { .. }), "{error:?}");
}

#[test]
fn non_string_catalog_names_cannot_bypass_ownership() {
    let error = load_error(&[
        (
            "pnpm-workspace.yaml",
            "include:\n  ./a.yaml: [catalogs.foo]\ncatalogs:\n  2024:\n    react: '1'\n",
        ),
        ("a.yaml", "catalogs:\n  2024:\n    react: '666'\n"),
    ]);
    assert!(matches!(error, IncludeError::SettingNotDelegated { .. }), "{error:?}");
}

#[test]
fn a_tagged_catalogs_value_is_checked_like_a_plain_one() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", "include:\n  ./a.yaml: []\n"),
        ("a.yaml", "catalogs: !x\n  evil:\n    react: '666'\n"),
    ]);
    assert!(matches!(error, IncludeError::SettingNotDelegated { .. }), "{error:?}");
}

#[test]
fn a_catalogs_value_that_is_not_a_mapping_is_rejected() {
    let error = load_error(&[
        ("pnpm-workspace.yaml", "include:\n  ./a.yaml: '*'\n"),
        ("a.yaml", "catalogs: [react]\n"),
    ]);
    assert!(matches!(error, IncludeError::ParseFragment { .. }), "{error:?}");
}

#[test]
fn merge_keys_cannot_define_a_reserved_setting() {
    let error = load_error(&[
        (
            "pnpm-workspace.yaml",
            "include:\n  ./security.yaml: [minimumReleaseAge]\n  ./a.yaml: '*'\n",
        ),
        ("security.yaml", ""),
        ("a.yaml", "defaults: &defaults\n  minimumReleaseAge: 0\n<<: *defaults\n"),
    ]);
    assert!(matches!(error, IncludeError::DuplicateSetting { .. }), "{error:?}");
}

#[test]
fn an_escaped_include_key_is_honored() {
    let dir = workspace(&[
        ("pnpm-workspace.yaml", "\"inc\\x6cude\":\n  ./deploy.yaml: [nodeLinker]\n"),
        ("deploy.yaml", "nodeLinker: hoisted\n"),
    ]);
    let document = load(dir.path()).expect("load");
    assert_eq!(local_owner(&document, &setting("nodeLinker")).as_deref(), Some("./deploy.yaml"));
}

#[test]
fn relative_paths_are_measured_from_the_including_fragment() {
    let dir = workspace(&[
        (
            "pnpm-workspace.yaml",
            "include:\n  ./patches/patches.yaml: [patchedDependencies, patchesDir]\n  ./config/settings.yaml: '*'\n",
        ),
        (
            "patches/patches.yaml",
            "patchesDir: .\npatchedDependencies:\n  foo@1.0.0: ./foo.patch\n  bar: bar.patch\n",
        ),
        (
            "config/settings.yaml",
            concat!(
                "packages:\n  - '../packages/*'\n  - '!../packages/skip'\n",
                "storeDir: ~/store\ncacheDir: /abs/cache\nmodulesDir: ${NM}/node_modules\n",
                "pnpmfile: ['./hooks.mjs']\nscriptShell: ./bin/sh\n",
                "catalog:\n  linked: link:../libs/linked\n  tarball: ./vendor/x.tgz\n  range: ^1.0.0\n",
                "overrides:\n  local: file:../vendor/local\n  ref: $ref\n",
                "  sibling: workspace:../packages/sibling\n",
            ),
        ),
    ]);
    let expected: yaml_serde::Value = yaml_serde::from_str(concat!(
        "patchesDir: patches\n",
        "patchedDependencies:\n  foo@1.0.0: ./patches/foo.patch\n  bar: patches/bar.patch\n",
        "packages:\n  - 'packages/*'\n  - '!packages/skip'\n",
        "storeDir: ~/store\ncacheDir: /abs/cache\nmodulesDir: ${NM}/node_modules\n",
        "pnpmfile: ['./config/hooks.mjs']\nscriptShell: ./config/bin/sh\n",
        "catalog:\n  linked: link:libs/linked\n  tarball: ./config/vendor/x.tgz\n  range: ^1.0.0\n",
        "overrides:\n  local: file:vendor/local\n  ref: $ref\n",
        "  sibling: workspace:./packages/sibling\n",
    ))
    .expect("expected settings parse");
    assert_eq!(merged(dir.path()), expected);
}

#[test]
fn a_bare_command_script_shell_is_not_a_path() {
    let dir = workspace(&[
        ("pnpm-workspace.yaml", "include:\n  ./config/shell.yaml: [scriptShell]\n"),
        ("config/shell.yaml", "scriptShell: bash\n"),
    ]);
    let expected: yaml_serde::Value =
        yaml_serde::from_str("scriptShell: bash\n").expect("expected settings parse");
    assert_eq!(merged(dir.path()), expected);
}

#[test]
fn paths_in_a_fragment_beside_the_root_manifest_are_kept() {
    let dir = workspace(&[
        ("pnpm-workspace.yaml", "include:\n  ./patches.yaml: [patchedDependencies]\n"),
        ("patches.yaml", "patchedDependencies:\n  foo: patches/foo.patch\n"),
    ]);
    let expected: yaml_serde::Value =
        yaml_serde::from_str("patchedDependencies:\n  foo: patches/foo.patch\n")
            .expect("expected settings parse");
    assert_eq!(merged(dir.path()), expected);
}

#[test]
fn paths_in_a_fragment_outside_the_workspace_are_measured_from_it() {
    let dir = workspace(&[
        ("ws/pnpm-workspace.yaml", "include:\n  ../shared/policy.yaml: [patchesDir]\n"),
        ("shared/policy.yaml", "patchesDir: patches\n"),
    ]);
    let expected: yaml_serde::Value =
        yaml_serde::from_str("patchesDir: ../shared/patches\n").expect("expected settings parse");
    assert_eq!(merged(&dir.path().join("ws")), expected);
}

#[test]
fn merging_at_a_revision_measures_paths_from_each_fragment() {
    let root = "include:\n  ./patches/patches.yaml: [patchedDependencies]\n";
    let text = super::merge_local_includes(root, Path::new("ws"), |path| {
        assert_eq!(path, Path::new("ws/./patches/patches.yaml"));
        Ok(Some("patchedDependencies:\n  foo: ./foo.patch\n".to_string()))
    })
    .expect("merge");
    let merged: yaml_serde::Value = yaml_serde::from_str(&text).expect("merged text parses");
    let expected: yaml_serde::Value =
        yaml_serde::from_str("patchedDependencies:\n  foo: ./patches/foo.patch\n")
            .expect("expected settings parse");
    assert_eq!(merged, expected);
}
