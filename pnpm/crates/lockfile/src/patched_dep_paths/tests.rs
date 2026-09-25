use super::{PatchedDepPathsStatus, check_patched_dep_paths};
use crate::Lockfile;
use pretty_assertions::assert_eq;

const CURRENT: &str = "aaaa1111";
const STALE: &str = "bbbb2222";

fn status(source: &str) -> PatchedDepPathsStatus {
    let lockfile: Lockfile = serde_saphyr::from_str(source).expect("parse lockfile");
    check_patched_dep_paths(&lockfile)
}

// A git / tarball / `file:` dependency records the version the patch was
// matched against on its `packages:` entry, since its depPath's version
// slot holds the reference instead.
const GIT_DEP_PATH: &str = "foo@git+file:///repo#0123456789012345678901234567890123456789";

fn git_lockfile(patched_dependencies: &str, package_version: &str, patch_hash: &str) -> String {
    format!(
        r"
lockfileVersion: '9.0'
{patched_dependencies}importers:
  .: {{}}
packages:
  {GIT_DEP_PATH}:
    resolution: {{type: git, repo: file:///repo, commit: '0123456789012345678901234567890123456789'}}
{package_version}snapshots:
  {GIT_DEP_PATH}(patch_hash={patch_hash}): {{}}
",
    )
}

#[test]
fn every_suffix_matching_patched_dependencies_is_up_to_date() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  is-positive@1.0.0: {CURRENT}
importers:
  .:
    dependencies:
      is-positive:
        specifier: 1.0.0
        version: 1.0.0(patch_hash={CURRENT})
packages:
  is-positive@1.0.0:
    resolution: {{integrity: sha512-fake}}
snapshots:
  is-positive@1.0.0(patch_hash={CURRENT}): {{}}
"
        )),
        PatchedDepPathsStatus::UpToDate,
    );
}

#[test]
fn a_lockfile_with_no_patches_is_up_to_date() {
    assert_eq!(
        status(
            r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      is-positive:
        specifier: 1.0.0
        version: 1.0.0
packages:
  is-positive@1.0.0:
    resolution: {integrity: sha512-fake}
snapshots:
  is-positive@1.0.0: {}
"
        ),
        PatchedDepPathsStatus::UpToDate,
    );
}

#[test]
fn an_importer_pinned_to_an_unrecorded_hash_is_stale() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  is-positive@1.0.0: {CURRENT}
importers:
  .:
    dependencies:
      is-positive:
        specifier: 1.0.0
        version: 1.0.0(patch_hash={STALE})
"
        )),
        PatchedDepPathsStatus::Stale,
    );
}

#[test]
fn a_stale_snapshot_key_is_stale() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  is-positive@1.0.0: {CURRENT}
importers:
  .: {{}}
snapshots:
  is-positive@1.0.0(patch_hash={STALE}): {{}}
"
        )),
        PatchedDepPathsStatus::Stale,
    );
}

#[test]
fn a_stale_dependency_edge_of_a_snapshot_is_stale() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  is-positive@1.0.0: {CURRENT}
importers:
  .: {{}}
snapshots:
  is-odd@3.0.1:
    dependencies:
      is-positive: 1.0.0(patch_hash={STALE})
"
        )),
        PatchedDepPathsStatus::Stale,
    );
}

#[test]
fn a_suffix_left_behind_after_every_patch_was_removed_is_stale() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
importers:
  .: {{}}
snapshots:
  is-positive@1.0.0(patch_hash={CURRENT}): {{}}
"
        )),
        PatchedDepPathsStatus::Stale,
    );
}

/// The patch hash sits in front of the peer suffix, so reading it means
/// looking past the peers rather than at the last parenthesized segment.
#[test]
fn a_peer_suffix_does_not_hide_the_patch_hash() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  react-dom@18.0.0: {CURRENT}
importers:
  .: {{}}
snapshots:
  react-dom@18.0.0(patch_hash={STALE})(react@18.0.0): {{}}
"
        )),
        PatchedDepPathsStatus::Stale,
    );
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  react-dom@18.0.0: {CURRENT}
importers:
  .: {{}}
snapshots:
  react-dom@18.0.0(patch_hash={CURRENT})(react@18.0.0): {{}}
"
        )),
        PatchedDepPathsStatus::UpToDate,
    );
}

/// An aliased edge names its target, so the package to look up is the one
/// the reference points at rather than the alias it is filed under.
#[test]
fn an_aliased_edge_resolves_to_the_package_it_points_at() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  is-positive@1.0.0: {CURRENT}
importers:
  .: {{}}
snapshots:
  is-odd@3.0.1:
    dependencies:
      positive: is-positive@1.0.0(patch_hash={STALE})
"
        )),
        PatchedDepPathsStatus::Stale,
    );
}

#[test]
fn a_hash_shared_by_several_patched_packages_is_up_to_date() {
    let other = "cccc3333";
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  is-positive@1.0.0: {CURRENT}
  is-odd@3.0.1: {other}
importers:
  .: {{}}
snapshots:
  is-positive@1.0.0(patch_hash={CURRENT}): {{}}
  is-odd@3.0.1(patch_hash={other}):
    dependencies:
      is-positive: 1.0.0(patch_hash={CURRENT})
"
        )),
        PatchedDepPathsStatus::UpToDate,
    );
}

/// `foo@1.0.0` and `foo@2.0.0` started out sharing one patch file, so both
/// recorded `shared`. `foo@2.0.0`'s patch has since diverged, but its suffix
/// still says `shared` — a hash the map does still contain, under the entry
/// for the *other* version. Matching the hash anywhere in the map would miss
/// this; it has to be matched against the entry for that package.
#[test]
fn a_hash_belonging_to_a_different_entry_is_stale() {
    let shared = "aaaa1111";
    let diverged = "dddd4444";
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  foo@1.0.0: {shared}
  foo@2.0.0: {diverged}
importers:
  .: {{}}
snapshots:
  foo@1.0.0(patch_hash={shared}): {{}}
  foo@2.0.0(patch_hash={shared}): {{}}
"
        )),
        PatchedDepPathsStatus::Stale,
    );
}

#[test]
fn several_entries_legitimately_sharing_one_patch_file_are_up_to_date() {
    let shared = "aaaa1111";
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  foo@1.0.0: {shared}
  foo@2.0.0: {shared}
importers:
  .: {{}}
snapshots:
  foo@1.0.0(patch_hash={shared}): {{}}
  foo@2.0.0(patch_hash={shared}): {{}}
"
        )),
        PatchedDepPathsStatus::UpToDate,
    );
}

#[test]
fn a_range_entry_covers_the_versions_it_matches() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  foo@^1.0.0: {CURRENT}
importers:
  .: {{}}
snapshots:
  foo@1.5.0(patch_hash={CURRENT}): {{}}
"
        )),
        PatchedDepPathsStatus::UpToDate,
    );
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  foo@^1.0.0: {CURRENT}
importers:
  .: {{}}
snapshots:
  foo@1.5.0(patch_hash={STALE}): {{}}
"
        )),
        PatchedDepPathsStatus::Stale,
    );
}

#[test]
fn a_patched_git_dependency_matches_on_its_recorded_version() {
    assert_eq!(
        status(&git_lockfile(
            &format!("patchedDependencies:\n  foo@1.0.0: {CURRENT}\n"),
            "    version: 1.0.0\n",
            CURRENT,
        )),
        PatchedDepPathsStatus::UpToDate,
    );
}

#[test]
fn a_stale_hash_on_a_patched_git_dependency_is_stale() {
    assert_eq!(
        status(&git_lockfile(
            &format!("patchedDependencies:\n  foo@1.0.0: {CURRENT}\n"),
            "    version: 1.0.0\n",
            STALE,
        )),
        PatchedDepPathsStatus::Stale,
    );
}

#[test]
fn a_range_entry_resolves_against_the_recorded_version_of_a_git_dependency() {
    assert_eq!(
        status(&git_lockfile(
            &format!("patchedDependencies:\n  foo@^1.0.0: {CURRENT}\n"),
            "    version: 1.5.0\n",
            STALE,
        )),
        PatchedDepPathsStatus::Stale,
    );
}

#[test]
fn a_suffix_left_on_an_unpatched_git_dependency_is_stale() {
    assert_eq!(
        status(&git_lockfile("", "    version: 1.0.0\n", CURRENT)),
        PatchedDepPathsStatus::Stale,
    );
}

/// Without a recorded version a versioned patch key cannot be matched either
/// way, so the segment cannot be judged. It is not evidence of a stale hash.
#[test]
fn a_versioned_patch_key_on_a_dependency_with_no_recorded_version_is_indeterminate() {
    assert_eq!(
        status(&git_lockfile(
            &format!("patchedDependencies:\n  foo@1.0.0: {CURRENT}\n"),
            "",
            STALE,
        )),
        PatchedDepPathsStatus::Indeterminate,
    );
}

/// A bare-name key matches whatever version the package turns out to have, so
/// it is judged on the hash alone even with no version to recover.
#[test]
fn a_bare_name_patch_key_is_judged_without_a_recorded_version() {
    assert_eq!(
        status(&git_lockfile(&format!("patchedDependencies:\n  foo: {CURRENT}\n"), "", CURRENT)),
        PatchedDepPathsStatus::UpToDate,
    );
    assert_eq!(
        status(&git_lockfile(&format!("patchedDependencies:\n  foo: {CURRENT}\n"), "", STALE)),
        PatchedDepPathsStatus::Stale,
    );
}

/// A key that does not resolve leaves its own package with nothing to be
/// judged against. The resolver reports the key itself.
#[test]
fn an_unparsable_patched_dependencies_key_is_indeterminate() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  foo@not-a-range: {CURRENT}
importers:
  .: {{}}
snapshots:
  foo@1.0.0(patch_hash={CURRENT}): {{}}
"
        )),
        PatchedDepPathsStatus::Indeterminate,
    );
}

#[test]
fn other_packages_are_still_judged_next_to_an_unparsable_key() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  foo@not-a-range: {CURRENT}
  bar@1.0.0: {CURRENT}
importers:
  .: {{}}
snapshots:
  foo@1.0.0(patch_hash={CURRENT}): {{}}
  bar@1.0.0(patch_hash={STALE}): {{}}
"
        )),
        PatchedDepPathsStatus::Stale,
    );
}

/// A registry-qualified key carries the semver behind its registry alias, and
/// gets no `packages:` `version:` entry because of it — pnpm's `parse` reports
/// that bare semver, which is what the patch keys are written against.
#[test]
fn a_registry_qualified_key_matches_on_the_semver_behind_its_alias() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  foo@1.1.0: {CURRENT}
importers:
  .: {{}}
packages:
  foo@work:1.1.0:
    resolution: {{integrity: sha512-fake}}
snapshots:
  foo@work:1.1.0(patch_hash={CURRENT}): {{}}
"
        )),
        PatchedDepPathsStatus::UpToDate,
    );
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  foo@1.1.0: {CURRENT}
importers:
  .: {{}}
packages:
  foo@work:1.1.0:
    resolution: {{integrity: sha512-fake}}
snapshots:
  foo@work:1.1.0(patch_hash={STALE}): {{}}
"
        )),
        PatchedDepPathsStatus::Stale,
    );
}

/// Rewriting only some of a package's `(patch_hash=...)` occurrences leaves
/// the rest pointing at a snapshot key that no longer exists. A registry
/// package's depPath still carries its version, so it is judged anyway.
#[test]
fn a_reference_to_a_missing_snapshot_is_judged_on_the_version_in_its_key() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  is-positive@1.0.0: {CURRENT}
importers:
  .: {{}}
snapshots:
  is-odd@3.0.1:
    dependencies:
      is-positive: 1.0.0(patch_hash={STALE})
"
        )),
        PatchedDepPathsStatus::Stale,
    );
}

/// A definite disagreement is knowledge, and a segment that cannot be judged
/// elsewhere does not take it away. Importers are walked before `snapshots:`,
/// so the segment that cannot be judged is met first.
#[test]
fn a_stale_segment_outranks_one_that_cannot_be_judged() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  foo@1.0.0: {CURRENT}
  is-positive@1.0.0: {CURRENT}
importers:
  .:
    dependencies:
      foo:
        specifier: git+file:///repo
        version: {GIT_DEP_PATH}(patch_hash={STALE})
snapshots:
  is-positive@1.0.0(patch_hash={STALE}): {{}}
"
        )),
        PatchedDepPathsStatus::Stale,
    );
}

/// A depPath whose version slot holds a resolution carries no version to match
/// a versioned patch key against, and its entry is the only other source.
#[test]
fn a_reference_to_a_missing_snapshot_with_no_version_in_its_key_is_indeterminate() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  foo@1.0.0: {CURRENT}
importers:
  .: {{}}
snapshots:
  is-odd@3.0.1:
    dependencies:
      foo: {GIT_DEP_PATH}(patch_hash={STALE})
"
        )),
        PatchedDepPathsStatus::Indeterminate,
    );
}

#[test]
fn a_snapshot_key_missing_the_segment_its_patch_calls_for_is_stale() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  is-positive@1.0.0: {CURRENT}
importers:
  .: {{}}
snapshots:
  is-positive@1.0.0: {{}}
"
        )),
        PatchedDepPathsStatus::Stale,
    );
}

#[test]
fn an_importer_reference_missing_the_segment_its_patch_calls_for_is_stale() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  is-positive@1.0.0: {CURRENT}
importers:
  .:
    dependencies:
      is-positive:
        specifier: 1.0.0
        version: 1.0.0
snapshots:
  is-positive@1.0.0(patch_hash={CURRENT}): {{}}
"
        )),
        PatchedDepPathsStatus::Stale,
    );
}

#[test]
fn a_dependency_edge_missing_the_segment_its_patch_calls_for_is_stale() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  is-positive@1.0.0: {CURRENT}
importers:
  .: {{}}
snapshots:
  is-odd@3.0.1:
    dependencies:
      positive: is-positive@1.0.0
  is-positive@1.0.0(patch_hash={CURRENT}): {{}}
"
        )),
        PatchedDepPathsStatus::Stale,
    );
}

#[test]
fn a_patched_git_dependency_missing_its_segment_is_stale() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  foo@1.0.0: {CURRENT}
importers:
  .: {{}}
packages:
  {GIT_DEP_PATH}:
    resolution: {{type: git, repo: file:///repo, commit: '0123456789012345678901234567890123456789'}}
    version: 1.0.0
snapshots:
  {GIT_DEP_PATH}: {{}}
",
        )),
        PatchedDepPathsStatus::Stale,
    );
}

#[test]
fn an_unsegmented_version_outside_the_patch_is_up_to_date() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  is-positive@1.0.0: {CURRENT}
importers:
  .:
    dependencies:
      is-positive:
        specifier: 2.0.0
        version: 2.0.0
snapshots:
  is-positive@2.0.0: {{}}
"
        )),
        PatchedDepPathsStatus::UpToDate,
    );
}

#[test]
fn a_patch_hash_marker_after_the_peers_cannot_be_judged() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
importers:
  .: {{}}
snapshots:
  foo@1.0.0(react@18.0.0)(patch_hash={STALE}): {{}}
"
        )),
        PatchedDepPathsStatus::Indeterminate,
    );
}

/// A peer's segment is that peer's own depPath, so a patched peer carries its
/// hash inside the peer segment of every package that sees it.
#[test]
fn a_patched_peer_nested_in_a_peer_segment_is_up_to_date() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  react@18.0.0: {CURRENT}
importers:
  .:
    dependencies:
      foo:
        specifier: 1.0.0
        version: 1.0.0(react@18.0.0(patch_hash={CURRENT}))
      react:
        specifier: 18.0.0
        version: 18.0.0(patch_hash={CURRENT})
snapshots:
  foo@1.0.0(react@18.0.0(patch_hash={CURRENT})):
    dependencies:
      react: 18.0.0(patch_hash={CURRENT})
  react@18.0.0(patch_hash={CURRENT}): {{}}
"
        )),
        PatchedDepPathsStatus::UpToDate,
    );
}

#[test]
fn a_stale_hash_nested_in_a_peer_segment_is_stale() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
patchedDependencies:
  react@18.0.0: {CURRENT}
importers:
  .: {{}}
snapshots:
  foo@1.0.0(react@18.0.0(patch_hash={STALE})): {{}}
"
        )),
        PatchedDepPathsStatus::Stale,
    );
}

/// An unmatched `)` must not let later parentheses rebalance the suffix and
/// hide a marker the leading segment does not hold.
#[test]
fn a_marker_behind_an_unmatched_parenthesis_cannot_be_judged() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
importers:
  .: {{}}
snapshots:
  foo@1.0.0(peer@1.0.0))(patch_hash={STALE})((x): {{}}
"
        )),
        PatchedDepPathsStatus::Indeterminate,
    );
}

fn nested_peers(depth: usize, innermost: &str) -> String {
    (0..depth).fold(innermost.to_string(), |inner, level| format!("p{level}@1.0.0({inner})"))
}

fn deeply_nested_lockfile(hash: &str) -> String {
    let key = nested_peers(64, &format!("react@18.0.0(patch_hash={hash})"));
    format!(
        r"
lockfileVersion: '9.0'
patchedDependencies:
  react@18.0.0: {CURRENT}
importers:
  .: {{}}
snapshots:
  {key}: {{}}
"
    )
}

/// Nesting depth alone is not a defect: a peer many levels down is judged
/// like any other.
#[test]
fn a_patched_peer_nested_many_levels_deep_is_judged() {
    assert_eq!(status(&deeply_nested_lockfile(CURRENT)), PatchedDepPathsStatus::UpToDate);
    assert_eq!(status(&deeply_nested_lockfile(STALE)), PatchedDepPathsStatus::Stale);
}

/// Text between segments would hide a marker from pnpm's `parse`, so the
/// suffix has to be segments back to back.
#[test]
fn text_between_suffix_segments_cannot_be_judged() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
importers:
  .: {{}}
snapshots:
  foo@1.0.0(patch_hash={STALE})junk(peer@1.0.0): {{}}
"
        )),
        PatchedDepPathsStatus::Indeterminate,
    );
}

#[test]
fn a_nested_peer_segment_that_is_not_a_dep_path_cannot_be_judged() {
    assert_eq!(
        status(&format!(
            r"
lockfileVersion: '9.0'
importers:
  .: {{}}
snapshots:
  a@1.0.0(b@1.0.0(patch_hash={STALE})junk): {{}}
"
        )),
        PatchedDepPathsStatus::Indeterminate,
    );
}
