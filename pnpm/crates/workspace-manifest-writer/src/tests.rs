//! Tests for the workspace-manifest catalog writer.
//!
//! Structural cases assert the parsed shape; the format-sensitive cases
//! assert byte-for-byte.

use std::{fs, path::PathBuf};

use indexmap::IndexMap;
use pnpm_catalogs_types::Catalogs;
use pnpm_package_manifest::PackageManifest;
use tempfile::TempDir;

use crate::{
    UpdateWorkspaceManifestOptions, WORKSPACE_MANIFEST_FILENAME, update_workspace_manifest,
};

fn catalogs(entries: &[(&str, &[(&str, &str)])]) -> Catalogs {
    entries
        .iter()
        .map(|(name, deps)| {
            let map = deps.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            (name.to_string(), map)
        })
        .collect()
}

/// Run `update_workspace_manifest` against `original` (when `Some`) and return
/// the resulting file contents, or `None` when no file exists afterward.
fn run(original: Option<&str>, updated: &Catalogs) -> Option<String> {
    run_with(
        original,
        &UpdateWorkspaceManifestOptions { updated_catalogs: Some(updated), ..Default::default() },
    )
}

fn run_with(original: Option<&str>, opts: &UpdateWorkspaceManifestOptions<'_>) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    update_workspace_manifest(dir.path(), opts).expect("update succeeds");
    fs::read_to_string(&path).ok()
}

/// [`run_with`] for the cleanup cases: merge `updated` (when `Some`), then
/// run the `catalogPrune` pass over `projects`.
fn run_cleanup(
    original: Option<&str>,
    updated: Option<&Catalogs>,
    projects: &[&PackageManifest],
) -> Option<String> {
    run_with(
        original,
        &UpdateWorkspaceManifestOptions {
            updated_catalogs: updated,
            catalog_prune: true,
            all_projects: projects,
            ..Default::default()
        },
    )
}

fn project(manifest: serde_json::Value) -> PackageManifest {
    PackageManifest::from_value(PathBuf::from("project/package.json"), manifest)
}

/// Run `set_config_dependencies` with a single entry against `original`
/// (when `Some`) and return the resulting file contents.
fn run_config_dep(original: Option<&str>, name: &str, specifier: &str) -> String {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::set_config_dependencies(dir.path(), [(name, specifier)]).expect("update succeeds");
    fs::read_to_string(&path).expect("file written")
}

/// Run `set_allow_builds` against `original` (when `Some`) and return the
/// resulting file contents (or `None` when no file exists afterward).
fn run_allow_builds(original: Option<&str>, entries: &[(&str, bool)]) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::set_allow_builds(dir.path(), entries.iter().copied()).expect("update succeeds");
    fs::read_to_string(&path).ok()
}

/// Run `scaffold_allow_builds` against `original` (when `Some`) and
/// return the resulting file contents (or `None` when no file exists
/// afterward).
fn run_scaffold_allow_builds(original: Option<&str>, names: &[&str]) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::scaffold_allow_builds(dir.path(), names.iter().copied()).expect("update succeeds");
    fs::read_to_string(&path).ok()
}

fn patched_deps(entries: &[(&str, &str)]) -> IndexMap<String, String> {
    entries.iter().map(|(key, value)| ((*key).to_string(), (*value).to_string())).collect()
}

/// Run `set_patched_dependencies` against `original` (when `Some`) and return
/// the resulting file contents.
fn run_patched_deps(original: Option<&str>, entries: &[(&str, &str)]) -> String {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::set_patched_dependencies(dir.path(), &patched_deps(entries)).expect("update succeeds");
    fs::read_to_string(&path).expect("file written")
}

fn run_patched_deps_path(original: Option<&str>, entries: &[(&str, &str)]) -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::set_patched_dependencies(dir.path(), &patched_deps(entries)).expect("update succeeds");
    (dir, path)
}

fn overrides(entries: &[(&str, &str)]) -> IndexMap<String, String> {
    entries.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// Run `set_overrides` against `original` (when `Some`) and return the
/// resulting file contents, or `None` when no file exists afterward.
fn run_overrides(original: Option<&str>, entries: &IndexMap<String, String>) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::set_overrides(
        dir.path(),
        entries.iter().map(|(key, value)| (key.as_str(), value.as_str())),
    )
    .expect("set_overrides succeeds");
    fs::read_to_string(&path).ok()
}

/// Run `set_audit_ignore_ghsas` against `original` (when `Some`) and return
/// the resulting file contents, or `None` when no file exists afterward.
fn run_ignore_ghsas(original: Option<&str>, ghsas: &[&str]) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    let owned: Vec<String> = ghsas.iter().map(ToString::to_string).collect();
    crate::set_audit_ignore_ghsas(dir.path(), &owned).expect("set_audit_ignore_ghsas succeeds");
    fs::read_to_string(&path).ok()
}

/// Run `remove_overrides` against `original` and return the resulting file
/// contents, or `None` when no file exists afterward.
fn run_remove_overrides(original: Option<&str>, selectors: &[&str]) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    let selectors: Vec<String> = selectors.iter().copied().map(ToString::to_string).collect();
    crate::remove_overrides(dir.path(), &selectors).expect("remove succeeds");
    fs::read_to_string(&path).ok()
}

fn run_allow_builds_clearing_legacy(
    original: Option<&str>,
    entries: &[(&str, bool)],
) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::set_allow_builds_clearing_legacy(dir.path(), entries.iter().copied())
        .expect("update succeeds");
    fs::read_to_string(&path).ok()
}

/// Run `set_minimum_release_age_excludes` against `original` and return the
/// resulting file contents, or `None` when no file exists afterward.
fn run_age_excludes(original: Option<&str>, excludes: &[&str]) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    let owned: Vec<String> = excludes.iter().map(ToString::to_string).collect();
    crate::set_minimum_release_age_excludes(dir.path(), &owned)
        .expect("set_minimum_release_age_excludes succeeds");
    fs::read_to_string(&path).ok()
}

// --- generic top-level field set/delete (pnpm config set / delete) ---

fn run_update_field(
    original: Option<&str>,
    key: &str,
    value: &serde_json::Value,
) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::update_manifest_field(&path, key, value).expect("update succeeds");
    fs::read_to_string(&path).ok()
}

/// Ported from upstream `removeCatalogs.test.ts`. The upstream tests
/// assert the parsed shape; these assert the format-preserving text the
/// pacquet writer produces for the same inputs.
mod remove_unused_catalogs {
    use super::{PackageManifest, catalogs, project, run_cleanup};

    /// TS: `remove the default catalog if it is empty`.
    #[test]
    fn removes_the_default_catalog_when_nothing_references_it() {
        let consumer = project(serde_json::json!({ "dependencies": { "foo": "^0.1.2" } }));
        let out = run_cleanup(Some("catalog:\n  foo: ^0.1.2\n"), None, &[&consumer]);
        assert_eq!(out, None, "an emptied manifest file must be deleted");
    }

    /// TS: `remove the unused default catalog`.
    #[test]
    fn removes_the_unused_default_catalog_entries() {
        let consumer = project(serde_json::json!({
            "dependencies": { "foo": "^0.1.2", "bar": "catalog:" },
        }));
        let out = run_cleanup(Some("catalog:\n  bar: 3.2.1\n  foo: ^0.1.2\n"), None, &[&consumer]);
        assert_eq!(out.as_deref(), Some("catalog:\n  bar: 3.2.1\n"));
    }

    /// TS: `remove the unused default catalog with catalogs`.
    #[test]
    fn removes_the_unused_entries_under_catalogs_default() {
        let consumer = project(serde_json::json!({
            "dependencies": { "foo": "^0.1.2", "bar": "catalog:" },
        }));
        let out = run_cleanup(
            Some("catalogs:\n  default:\n    bar: 3.2.1\n    foo: ^0.1.2\n"),
            None,
            &[&consumer],
        );
        assert_eq!(out.as_deref(), Some("catalogs:\n  default:\n    bar: 3.2.1\n"));
    }

    /// TS: `remove the unused named catalog`.
    #[test]
    fn removes_an_entirely_unused_named_catalog() {
        let consumer = project(serde_json::json!({
            "dependencies": { "abc": "0.1.2", "def": "catalog:bar" },
        }));
        let out = run_cleanup(
            Some("catalogs:\n  foo:\n    abc: 0.1.2\n  bar:\n    def: 3.2.1\n"),
            None,
            &[&consumer],
        );
        assert_eq!(out.as_deref(), Some("catalogs:\n  bar:\n    def: 3.2.1\n"));
    }

    /// TS: `remove all unused named catalogs` (the final cleanup that
    /// empties every named catalog and deletes the file).
    #[test]
    fn removes_the_file_when_every_named_catalog_empties() {
        let consumer = project(serde_json::json!({ "dependencies": { "def": "3.2.1" } }));
        let out = run_cleanup(
            Some("catalogs:\n  bar:\n    def: 3.2.1\n  foo:\n    ghi: 7.8.9\n"),
            None,
            &[&consumer],
        );
        assert_eq!(out, None, "an emptied manifest file must be deleted");
    }

    /// TS: `same pkg with different version` — the same package name may
    /// be referenced per catalog; each referenced entry survives.
    #[test]
    fn keeps_the_same_package_in_each_referenced_catalog() {
        let consumer = project(serde_json::json!({
            "dependencies": { "def": "catalog:bar", "ghi": "catalog:foo", "abc": "catalog:foo" },
            "optionalDependencies": { "abc": "catalog:bar" },
        }));
        let original = "catalogs:\n  foo:\n    abc: 0.1.2\n    ghi: 7.8.9\n  bar:\n    abc: 1.2.3\n    def: 3.2.1\n";
        let out = run_cleanup(Some(original), None, &[&consumer]);
        assert_eq!(out.as_deref(), Some(original));
    }

    /// TS: `update catalogs and remove catalog` — one write both merges
    /// updated entries and drops the unreferenced ones.
    #[test]
    fn updates_catalogs_and_cleans_up_in_one_write() {
        let consumer = project(serde_json::json!({
            "dependencies": { "def": "catalog:bar", "ghi": "catalog:foo" },
        }));
        let out = run_cleanup(
            Some("catalogs:\n  foo:\n    abc: 0.1.2\n    ghi: 7.8.9\n  bar:\n    def: 3.2.1\n"),
            Some(&catalogs(&[("foo", &[("ghi", "7.9.9")])])),
            &[&consumer],
        );
        assert_eq!(
            out.as_deref(),
            Some("catalogs:\n  foo:\n    ghi: 7.9.9\n  bar:\n    def: 3.2.1\n"),
        );
    }

    /// TS: `when allProjects is undefined should not cleanup unused
    /// catalogs`.
    #[test]
    fn skips_cleanup_without_projects() {
        let projects: [&PackageManifest; 0] = [];
        let out = run_cleanup(
            Some("catalogs:\n  foo:\n    abc: 0.1.2\n    ghi: 7.8.9\n  bar:\n    def: 3.2.1\n"),
            Some(&catalogs(&[("foo", &[("ghi", "7.9.9")])])),
            &projects,
        );
        assert_eq!(
            out.as_deref(),
            Some("catalogs:\n  foo:\n    abc: 0.1.2\n    ghi: 7.9.9\n  bar:\n    def: 3.2.1\n"),
        );
    }

    #[test]
    fn prunes_a_flow_style_catalogs_mapping() {
        let consumer = project(serde_json::json!({ "dependencies": { "def": "catalog:bar" } }));
        let original = "catalogs: { foo: { abc: 0.1.2 }, bar: { def: 3.2.1 } }\n";
        let out = run_cleanup(Some(original), None, &[&consumer]);
        assert_eq!(out.as_deref(), Some("catalogs: { bar: { def: 3.2.1 } }\n"));
    }

    #[test]
    fn prunes_the_entries_of_a_flow_style_named_catalog() {
        let consumer = project(serde_json::json!({ "dependencies": { "abc": "catalog:foo" } }));
        let original = "catalogs: { foo: { abc: 0.1.2, ghi: 7.8.9 } }\n";
        let out = run_cleanup(Some(original), None, &[&consumer]);
        assert_eq!(out.as_deref(), Some("catalogs: { foo: { abc: 0.1.2 } }\n"));
    }

    /// TS: `keep catalogs referenced only in workspace overrides`.
    #[test]
    fn keeps_entries_referenced_only_by_workspace_overrides() {
        let consumer = project(serde_json::json!({ "dependencies": { "zoo": "^1.0.0" } }));
        let original = "catalog:\n  foo: 1.0.0\n\
            catalogs:\n  bar:\n    '@scope/def': 2.0.0\n\
            overrides:\n  foo: 'catalog:'\n  '@scope/parent@1>@scope/def': 'catalog:bar'\n";
        let out = run_cleanup(Some(original), None, &[&consumer]);
        assert_eq!(out.as_deref(), Some(original));
    }

    /// TS: `remove catalogs unused by dependencies and workspace
    /// overrides`.
    #[test]
    fn removes_entries_unreferenced_by_dependencies_and_overrides() {
        let consumer = project(serde_json::json!({ "dependencies": { "zoo": "^1.0.0" } }));
        let out = run_cleanup(
            Some(
                "catalog:\n  foo: 1.0.0\n  unusedDefault: 2.0.0\n\
                 catalogs:\n  bar:\n    def: 2.0.0\n    unusedNamed: 3.0.0\n\
                 overrides:\n  foo: 'catalog:'\n  def: 'catalog:bar'\n",
            ),
            None,
            &[&consumer],
        );
        assert_eq!(
            out.as_deref(),
            Some(
                "catalog:\n  foo: 1.0.0\n\
                 catalogs:\n  bar:\n    def: 2.0.0\n\
                 overrides:\n  foo: 'catalog:'\n  def: 'catalog:bar'\n",
            ),
        );
    }
}

/// The `minimumReleaseAgeExcludePrune` pass: entries of
/// `minimumReleaseAgeExclude` are pruned against the versions the
/// freshly resolved lockfile records.
mod minimum_release_age_exclude_prune {
    use crate::ResolvedPackageVersions;

    use super::{UpdateWorkspaceManifestOptions, run_with};

    fn resolved(entries: &[(&str, &[&str])]) -> ResolvedPackageVersions {
        entries
            .iter()
            .map(|(name, versions)| {
                (name.to_string(), versions.iter().map(ToString::to_string).collect())
            })
            .collect()
    }

    fn run_age_cleanup(
        original: Option<&str>,
        resolved: Option<&ResolvedPackageVersions>,
    ) -> Option<String> {
        run_with(
            original,
            &UpdateWorkspaceManifestOptions {
                prune_minimum_release_age_excludes: true,
                resolved_package_versions: resolved,
                ..Default::default()
            },
        )
    }

    #[test]
    fn drops_a_versioned_entry_whose_version_is_no_longer_resolved() {
        let original = "packages:\n  - '*'\nminimumReleaseAgeExclude:\n  - foo@1.0.0\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[("foo", &["2.0.0"])])));
        assert_eq!(out.as_deref(), Some("packages:\n  - '*'\n"));
    }

    #[test]
    fn keeps_a_versioned_entry_whose_version_is_resolved() {
        let original = "minimumReleaseAgeExclude:\n  - foo@1.0.0\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[("foo", &["1.0.0"])])));
        assert_eq!(out.as_deref(), Some(original));
    }

    #[test]
    fn rewrites_a_narrowed_version_union_canonically() {
        let original = "minimumReleaseAgeExclude:\n  - foo@1.0.0 || 2.0.0\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[("foo", &["2.0.0"])])));
        assert_eq!(out.as_deref(), Some("minimumReleaseAgeExclude:\n  - foo@2.0.0\n"));
    }

    #[test]
    fn keeps_a_union_entry_verbatim_when_every_version_is_resolved() {
        let original = "minimumReleaseAgeExclude:\n  - foo@2.0.0 || 1.0.0\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[("foo", &["1.0.0", "2.0.0"])])));
        assert_eq!(out.as_deref(), Some(original), "no version was dropped, so no rewrite");
    }

    #[test]
    fn keeps_a_bare_name_when_the_package_is_resolved() {
        let original = "minimumReleaseAgeExclude:\n  - foo\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[("foo", &["2.0.0"])])));
        assert_eq!(out.as_deref(), Some(original));
    }

    /// A package resolved only via a non-semver source (git, tarball,
    /// `file:`) registers with an empty version set: its bare-name entry
    /// survives (the package is still a dependency) but its versioned
    /// entries are pruned (no exact version can be confirmed).
    #[test]
    fn keeps_the_bare_name_but_prunes_versions_of_a_non_semver_only_package() {
        let original = "minimumReleaseAgeExclude:\n  - foo\n  - foo@1.0.0\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[("foo", &[])])));
        assert_eq!(out.as_deref(), Some("minimumReleaseAgeExclude:\n  - foo\n"));
    }

    #[test]
    fn drops_a_bare_name_when_the_package_is_absent() {
        let original = "minimumReleaseAgeExclude:\n  - foo\n  - bar@1.0.0\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[("bar", &["1.0.0"])])));
        assert_eq!(out.as_deref(), Some("minimumReleaseAgeExclude:\n  - bar@1.0.0\n"));
    }

    #[test]
    fn keeps_a_glob_entry_with_no_match() {
        let original = "minimumReleaseAgeExclude:\n  - '@babel/*'\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[])));
        assert_eq!(out.as_deref(), Some(original));
    }

    #[test]
    fn removes_the_file_when_the_emptied_block_was_the_only_key() {
        let original = "minimumReleaseAgeExclude:\n  - foo@1.0.0\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[])));
        assert_eq!(out, None, "an emptied manifest file must be deleted");
    }

    #[test]
    fn skips_cleanup_without_resolved_versions() {
        let original = "minimumReleaseAgeExclude:\n  - foo@1.0.0\n";
        let out = run_age_cleanup(Some(original), None);
        assert_eq!(out.as_deref(), Some(original));
    }

    #[test]
    fn keeps_an_unparsable_entry_verbatim() {
        let original = "minimumReleaseAgeExclude:\n  - foo@>=1.0.0\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[])));
        assert_eq!(out.as_deref(), Some(original));
    }

    /// An empty list has nothing to prune; removing the block would diverge
    /// from the TypeScript implementation, which leaves it untouched.
    #[test]
    fn keeps_an_empty_list_verbatim() {
        let original = "minimumReleaseAgeExclude: []\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[])));
        assert_eq!(out.as_deref(), Some(original));
    }
}

/// The `trustPolicyExcludePrune` pass: entries of `trustPolicyExclude`
/// are pruned against the versions the freshly resolved lockfile records.
mod trust_policy_exclude_prune {
    use crate::ResolvedPackageVersions;

    use super::{UpdateWorkspaceManifestOptions, run_with};

    fn resolved(entries: &[(&str, &[&str])]) -> ResolvedPackageVersions {
        entries
            .iter()
            .map(|(name, versions)| {
                (name.to_string(), versions.iter().map(ToString::to_string).collect())
            })
            .collect()
    }

    fn run_trust_cleanup(
        original: Option<&str>,
        resolved: Option<&ResolvedPackageVersions>,
    ) -> Option<String> {
        run_with(
            original,
            &UpdateWorkspaceManifestOptions {
                prune_trust_policy_excludes: true,
                resolved_package_versions: resolved,
                ..Default::default()
            },
        )
    }

    #[test]
    fn drops_a_versioned_entry_whose_version_is_no_longer_resolved() {
        let original = "packages:\n  - '*'\ntrustPolicyExclude:\n  - foo@1.0.0\n";
        let out = run_trust_cleanup(Some(original), Some(&resolved(&[("foo", &["2.0.0"])])));
        assert_eq!(out.as_deref(), Some("packages:\n  - '*'\n"));
    }

    #[test]
    fn keeps_a_versioned_entry_whose_version_is_resolved() {
        let original = "trustPolicyExclude:\n  - foo@1.0.0\n";
        let out = run_trust_cleanup(Some(original), Some(&resolved(&[("foo", &["1.0.0"])])));
        assert_eq!(out.as_deref(), Some(original));
    }

    #[test]
    fn rewrites_a_narrowed_version_union_canonically() {
        let original = "trustPolicyExclude:\n  - foo@1.0.0 || 2.0.0\n";
        let out = run_trust_cleanup(Some(original), Some(&resolved(&[("foo", &["2.0.0"])])));
        assert_eq!(out.as_deref(), Some("trustPolicyExclude:\n  - foo@2.0.0\n"));
    }

    #[test]
    fn keeps_a_union_entry_verbatim_when_every_version_is_resolved() {
        let original = "trustPolicyExclude:\n  - foo@2.0.0 || 1.0.0\n";
        let out =
            run_trust_cleanup(Some(original), Some(&resolved(&[("foo", &["1.0.0", "2.0.0"])])));
        assert_eq!(out.as_deref(), Some(original), "no version was dropped, so no rewrite");
    }

    #[test]
    fn drops_a_bare_name_when_the_package_is_absent() {
        let original = "trustPolicyExclude:\n  - foo\n  - bar@1.0.0\n";
        let out = run_trust_cleanup(Some(original), Some(&resolved(&[("bar", &["1.0.0"])])));
        assert_eq!(out.as_deref(), Some("trustPolicyExclude:\n  - bar@1.0.0\n"));
    }

    #[test]
    fn keeps_a_glob_entry_with_no_match() {
        let original = "trustPolicyExclude:\n  - '@babel/*'\n";
        let out = run_trust_cleanup(Some(original), Some(&resolved(&[])));
        assert_eq!(out.as_deref(), Some(original));
    }

    #[test]
    fn removes_the_file_when_the_emptied_block_was_the_only_key() {
        let original = "trustPolicyExclude:\n  - foo@1.0.0\n";
        let out = run_trust_cleanup(Some(original), Some(&resolved(&[])));
        assert_eq!(out, None, "an emptied manifest file must be deleted");
    }

    #[test]
    fn skips_cleanup_without_resolved_versions() {
        let original = "trustPolicyExclude:\n  - foo@1.0.0\n";
        let out = run_trust_cleanup(Some(original), None);
        assert_eq!(out.as_deref(), Some(original));
    }

    #[test]
    fn keeps_an_unparsable_entry_verbatim() {
        let original = "trustPolicyExclude:\n  - foo@>=1.0.0\n";
        let out = run_trust_cleanup(Some(original), Some(&resolved(&[])));
        assert_eq!(out.as_deref(), Some(original));
    }

    #[test]
    fn prunes_both_exclude_lists_when_both_settings_are_on() {
        let original =
            "minimumReleaseAgeExclude:\n  - foo@1.0.0\ntrustPolicyExclude:\n  - bar@1.0.0\n";
        let out = run_with(
            Some(original),
            &UpdateWorkspaceManifestOptions {
                prune_minimum_release_age_excludes: true,
                prune_trust_policy_excludes: true,
                resolved_package_versions: Some(&resolved(&[("foo", &["1.0.0"])])),
                ..Default::default()
            },
        );
        assert_eq!(out.as_deref(), Some("minimumReleaseAgeExclude:\n  - foo@1.0.0\n"));
    }

    /// An empty list has nothing to prune; removing the block would diverge
    /// from the TypeScript implementation, which leaves it untouched.
    #[test]
    fn keeps_an_empty_list_verbatim() {
        let original = "trustPolicyExclude: []\n";
        let out = run_trust_cleanup(Some(original), Some(&resolved(&[])));
        assert_eq!(out.as_deref(), Some(original));
    }

    #[test]
    fn keeps_a_surviving_entry_trailing_comment_when_another_entry_is_pruned() {
        let original = "trustPolicyExclude:\n  - foo@1.0.0 # trusted fork\n  - bar@2.0.0\n";
        let out = run_trust_cleanup(Some(original), Some(&resolved(&[("foo", &["1.0.0"])])));
        assert_eq!(out.as_deref(), Some("trustPolicyExclude:\n  - foo@1.0.0 # trusted fork\n"));
    }

    /// A narrowed entry loses its own comment, matching the TypeScript
    /// writer's node reuse.
    #[test]
    fn keeps_a_surviving_entry_comment_when_a_narrowed_entry_is_rewritten() {
        let original =
            "trustPolicyExclude:\n  - foo@1.0.0 || 2.0.0 # both audited\n  - bar@1.0.0 # pinned\n";
        let out = run_trust_cleanup(
            Some(original),
            Some(&resolved(&[("foo", &["2.0.0"]), ("bar", &["1.0.0"])])),
        );
        assert_eq!(
            out.as_deref(),
            Some("trustPolicyExclude:\n  - foo@2.0.0\n  - bar@1.0.0 # pinned\n"),
        );
    }

    #[test]
    fn keeps_a_standalone_comment_above_the_list() {
        let original =
            "trustPolicyExclude:\n  # pinned after the audit\n  - foo@1.0.0\n  - bar@2.0.0\n";
        let out = run_trust_cleanup(Some(original), Some(&resolved(&[("foo", &["1.0.0"])])));
        assert_eq!(
            out.as_deref(),
            Some("trustPolicyExclude:\n  # pinned after the audit\n  - foo@1.0.0\n"),
        );
    }

    /// A comment between two entries belongs to the entry below it, like the
    /// TypeScript writer, so pruning the entry above leaves it in place.
    #[test]
    fn keeps_a_comment_between_entries_when_the_entry_above_is_pruned() {
        let original = "trustPolicyExclude:\n  - foo@1.0.0\n  # reason for bar\n  - bar@2.0.0\n";
        let out = run_trust_cleanup(Some(original), Some(&resolved(&[("bar", &["2.0.0"])])));
        assert_eq!(
            out.as_deref(),
            Some("trustPolicyExclude:\n  # reason for bar\n  - bar@2.0.0\n"),
        );
    }

    #[test]
    fn drops_a_comment_between_entries_with_the_entry_below_it() {
        let original = "trustPolicyExclude:\n  - foo@1.0.0\n  # reason for bar\n  - bar@2.0.0\n";
        let out = run_trust_cleanup(Some(original), Some(&resolved(&[("foo", &["1.0.0"])])));
        assert_eq!(out.as_deref(), Some("trustPolicyExclude:\n  - foo@1.0.0\n"));
    }

    /// A block scalar's body can hold `#`-leading lines that are its value
    /// rather than comments, so such a block is re-rendered whole instead of
    /// reconciled line by line, which would drop those lines with the entry
    /// below them and widen the exclude to the bare `*`.
    #[test]
    fn keeps_a_block_scalar_entry_value_when_another_entry_is_pruned() {
        let original = "trustPolicyExclude:\n  - >-\n    *\n    # note\n  - bar@2.0.0\n";
        let out = run_trust_cleanup(Some(original), Some(&resolved(&[])));
        assert_eq!(out.as_deref(), Some("trustPolicyExclude:\n  - '* # note'\n"));
    }

    #[test]
    fn keeps_a_blank_line_between_entries_when_the_entry_above_is_pruned() {
        let original = "trustPolicyExclude:\n  - foo@1.0.0\n\n  - bar@2.0.0\n";
        let out = run_trust_cleanup(Some(original), Some(&resolved(&[("bar", &["2.0.0"])])));
        assert_eq!(out.as_deref(), Some("trustPolicyExclude:\n\n  - bar@2.0.0\n"));
    }
}

fn run_prune_allow_builds(original: Option<&str>, resolved: &[&str]) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("pnpm-workspace.yaml");
    if let Some(text) = original {
        std::fs::write(&path, text).expect("write manifest");
    }
    let mut resolved_map = std::collections::BTreeMap::new();
    for name in resolved {
        resolved_map.insert(name.to_string(), std::collections::BTreeSet::new());
    }
    crate::update_workspace_manifest(
        dir.path(),
        &crate::UpdateWorkspaceManifestOptions {
            prune_allow_builds: true,
            resolved_package_versions: Some(&resolved_map),
            ..Default::default()
        },
    )
    .expect("update succeeds");
    path.exists().then(|| std::fs::read_to_string(&path).expect("read manifest"))
}

/// Every writer edits a hand-written single-line flow collection in place,
/// matching what the TypeScript writer's yaml library emits, and refuses a
/// multi-line one rather than dropping the comments between its entries.
mod flow_style {
    use super::{
        TempDir, UpdateWorkspaceManifestOptions, WORKSPACE_MANIFEST_FILENAME, catalogs, fs, run,
        run_age_excludes, run_allow_builds, run_config_dep, run_ignore_ghsas, run_patched_deps,
        run_remove_overrides, run_scaffold_allow_builds, update_workspace_manifest,
    };

    #[test]
    fn catalog_entry_is_added_to_a_flow_mapping() {
        let out = run(
            Some("catalog: { foo: ^1.0.0 }\n"),
            &catalogs(&[("default", &[("bar", "^2.0.0")])]),
        );
        assert_eq!(out.as_deref(), Some("catalog: { bar: ^2.0.0, foo: ^1.0.0 }\n"));
    }

    #[test]
    fn catalog_entry_is_updated_in_a_flow_mapping() {
        let out = run(
            Some("catalog: { foo: ^1.0.0 } # pins\n"),
            &catalogs(&[("default", &[("foo", "^2.0.0")])]),
        );
        assert_eq!(out.as_deref(), Some("catalog: { foo: ^2.0.0 } # pins\n"));
    }

    #[test]
    fn named_catalog_entry_is_added_to_a_nested_flow_mapping() {
        let out = run(
            Some("catalogs: { myCatalog: { foo: ^1.0.0 } }\n"),
            &catalogs(&[("myCatalog", &[("bar", "^2.0.0")])]),
        );
        assert_eq!(out.as_deref(), Some("catalogs: { myCatalog: { bar: ^2.0.0, foo: ^1.0.0 } }\n"));
    }

    #[test]
    fn a_new_named_catalog_is_added_to_a_flow_catalogs_mapping() {
        let out = run(
            Some("catalogs: { myCatalog: { foo: ^1.0.0 } }\n"),
            &catalogs(&[("newCatalog", &[("bar", "^2.0.0")])]),
        );
        assert_eq!(
            out.as_deref(),
            Some("catalogs: { myCatalog: { foo: ^1.0.0 }, newCatalog: { bar: ^2.0.0 } }\n"),
        );
    }

    #[test]
    fn config_dependency_is_added_to_a_flow_mapping() {
        let out = run_config_dep(Some("configDependencies: { foo: 1.0.0 }\n"), "bar", "2.0.0");
        assert_eq!(out, "configDependencies: { bar: 2.0.0, foo: 1.0.0 }\n");
    }

    #[test]
    fn allow_build_is_added_to_a_flow_mapping() {
        let out = run_allow_builds(Some("allowBuilds: { foo: true }\n"), &[("bar", false)]);
        assert_eq!(out.as_deref(), Some("allowBuilds: { bar: false, foo: true }\n"));
    }

    #[test]
    fn allow_build_is_updated_in_a_flow_mapping() {
        let out = run_allow_builds(Some("allowBuilds: { foo: true }\n"), &[("foo", false)]);
        assert_eq!(out.as_deref(), Some("allowBuilds: { foo: false }\n"));
    }

    #[test]
    fn undecided_allow_build_is_added_to_a_flow_mapping() {
        let out = run_scaffold_allow_builds(Some("allowBuilds: { foo: true }\n"), &["bar"]);
        assert_eq!(
            out.as_deref(),
            Some("allowBuilds: { bar: set this to true or false, foo: true }\n"),
        );
    }

    #[test]
    fn undecided_allow_build_leaves_a_decided_flow_entry_alone() {
        let original = "allowBuilds: { foo: true }\n";
        let out = run_scaffold_allow_builds(Some(original), &["foo"]);
        assert_eq!(out.as_deref(), Some(original));
    }

    #[test]
    fn patched_dependency_is_added_to_a_flow_mapping() {
        let out = run_patched_deps(
            Some("patchedDependencies: { foo: patches/foo.patch }\n"),
            &[("foo", "patches/foo.patch"), ("bar", "patches/bar.patch")],
        );
        assert_eq!(
            out,
            "patchedDependencies: { bar: patches/bar.patch, foo: patches/foo.patch }\n",
        );
    }

    #[test]
    fn omitted_patched_dependency_is_dropped_from_a_flow_mapping() {
        let out = run_patched_deps(
            Some("patchedDependencies: { foo: patches/foo.patch, bar: patches/bar.patch }\n"),
            &[("bar", "patches/bar.patch")],
        );
        assert_eq!(out, "patchedDependencies: { bar: patches/bar.patch }\n");
    }

    #[test]
    fn minimum_release_age_excludes_stay_a_flow_sequence() {
        let out = run_age_excludes(
            Some("minimumReleaseAgeExclude: [foo@1.0.0]\n"),
            &["foo@1.0.0", "bar@2.0.0"],
        );
        assert_eq!(out.as_deref(), Some("minimumReleaseAgeExclude: [ foo@1.0.0, bar@2.0.0 ]\n"));
    }

    #[test]
    fn ignore_ghsas_stay_a_flow_sequence_under_a_block_audit_config() {
        let out = run_ignore_ghsas(
            Some("auditConfig:\n  ignoreGhsas: [GHSA-aaaa-bbbb-cccc, GHSA-dddd-eeee-ffff]\n"),
            &["GHSA-gggg-hhhh-iiii"],
        );
        assert_eq!(out.as_deref(), Some("auditConfig:\n  ignoreGhsas: [ GHSA-gggg-hhhh-iiii ]\n"));
    }

    #[test]
    fn ignore_ghsas_are_added_to_a_flow_audit_config() {
        let out = run_ignore_ghsas(Some("auditConfig: {}\n"), &["GHSA-aaaa-bbbb-cccc"]);
        assert_eq!(out.as_deref(), Some("auditConfig: { ignoreGhsas: [ GHSA-aaaa-bbbb-cccc ] }\n"));
    }

    #[test]
    fn a_multiline_flow_mapping_is_refused_rather_than_flattened() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
        let original = "allowBuilds: {\n  foo: true, # decided\n}\n";
        fs::write(&path, original).expect("seed manifest");

        let err = crate::set_allow_builds(dir.path(), [("bar", true)])
            .expect_err("must refuse a multi-line inline allowBuilds block");

        assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
        assert_eq!(fs::read_to_string(&path).expect("read manifest"), original);
    }

    #[test]
    fn a_multiline_flow_sequence_is_refused_rather_than_rewritten() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
        let original = "minimumReleaseAgeExclude: [\n  foo@1.0.0, # pinned\n  bar@2.0.0,\n]\n";
        fs::write(&path, original).expect("seed manifest");

        let err = crate::set_minimum_release_age_excludes(dir.path(), &["baz@3.0.0".to_string()])
            .expect_err("must refuse a multi-line inline sequence");

        assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
        assert_eq!(fs::read_to_string(&path).expect("read manifest"), original);
    }

    #[test]
    fn a_multiline_flow_block_is_dropped_whole_when_it_empties() {
        let original = "packages:\n  - '*'\noverrides: {\n  foo: link:../foo, # pinned\n}\n";
        let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
        assert_eq!(out, "packages:\n  - '*'\n");
    }

    #[test]
    fn a_multiline_flow_block_is_deleted_whole_by_config_delete() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
        fs::write(&path, "overrides: {\n  foo: 1.0.0, # pinned\n}\npackages:\n  - '*'\n")
            .expect("seed manifest");

        crate::update_manifest_field(&path, "overrides", &serde_json::Value::Null)
            .expect("update_manifest_field succeeds");

        assert_eq!(fs::read_to_string(&path).expect("read manifest"), "packages:\n  - '*'\n");
    }

    /// A block whose value has the wrong shape for its setting never reaches
    /// the writers: the typed parse rejects the manifest first.
    #[test]
    fn a_flow_collection_of_the_wrong_kind_fails_to_parse() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
        let original = "allowBuilds: [ foo ]\n";
        fs::write(&path, original).expect("seed manifest");

        let err = crate::set_allow_builds(dir.path(), [("bar", true)])
            .expect_err("must refuse a sequence where allowBuilds expects a mapping");

        assert!(matches!(err, crate::UpdateWorkspaceManifestError::Parse { .. }));
        assert_eq!(fs::read_to_string(&path).expect("read manifest"), original);
    }

    #[test]
    fn a_whole_document_flow_mapping_is_refused_rather_than_corrupted() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
        // The keys of a document written as one flow mapping are not
        // top-level lines, so no splice — nor a new top-level block — can
        // address them.
        let original = "{ overrides: { foo: 1.0.0 } }\n";
        fs::write(&path, original).expect("seed manifest");

        let err = crate::set_overrides(dir.path(), [("bar", "2.0.0")])
            .expect_err("must refuse a whole-document flow mapping");

        assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
        assert_eq!(fs::read_to_string(&path).expect("read manifest"), original);
    }

    #[test]
    fn an_aliased_block_is_refused_rather_than_corrupted() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
        let original = "catalog: &pins { foo: ^1.0.0 }\ncatalogs: { other: *pins }\n";
        fs::write(&path, original).expect("seed manifest");

        let err = update_workspace_manifest(
            dir.path(),
            &UpdateWorkspaceManifestOptions {
                updated_catalogs: Some(&catalogs(&[("other", &[("bar", "^2.0.0")])])),
                ..Default::default()
            },
        )
        .expect_err("must refuse an aliased catalog block");

        assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
        assert_eq!(fs::read_to_string(&path).expect("read manifest"), original);
    }
}

mod workspace_settings;

mod behavior;

mod configuration;

mod files;

mod security;

mod manifests;

mod dependencies;

mod integrity;
