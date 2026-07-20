# Test Porting Plan

Scope: Stage 1 from pnpm/pacquet#299. The target behavior is `pacquet install --frozen-lockfile` matching `pnpm install --frozen-lockfile`. The TypeScript repo is the source of truth. This Rust repo should port these tests before or alongside the corresponding behavior.

Unless otherwise noted, TypeScript paths are relative to the TypeScript repo root, and Rust commands are run from this Rust repo root. Line numbers point at the TypeScript tests as found during this audit. Some tests are not frozen-lockfile tests themselves, but they exercise shared code or invariants that the frozen/headless installer must preserve.

**Do not stop at the per-package `test/` directories.** Each TypeScript package has its own unit and integration tests under directories such as `installing/deps-installer/test/`, `installing/deps-restorer/test/`, `network/auth-header/test/`, and `config/reader/test/`. In addition to those, the upstream repo has a top-level **`pnpm/test/`** directory. That directory contains CLI-level, end-to-end integration tests for almost every feature in pnpm, including install, monorepo and workspaces, lifecycle scripts, lockfile, global virtual store, runtime install, publish, link, store, prune, audit, dedupe, exec, and run. These tests drive the real `pnpm` CLI binary and exercise behaviors that the per-package tests do not, such as end-to-end config resolution, exit codes, stdout and stderr formatting, and multi-step CLI flows. When porting any feature, you must:

1. Look in the per-package `test/` directory for that feature's package, **and**
2. Look in `pnpm/test/` for matching CLI-level coverage (e.g. `pnpm/test/install/`, `pnpm/test/monorepo/`, `pnpm/test/install/lifecycleScripts.ts`, `pnpm/test/install/globalVirtualStore.ts`, `pnpm/test/install/runtimeOnFail.ts`, etc.).

This plan already cites a handful of `pnpm/test/...` files inline next to the feature they belong to, but those citations are not exhaustive. Treat `pnpm/test/` as a parallel test tree that must be audited for every feature you port, not just the ones it is already mentioned under. Skipping `pnpm/test/` is the single most common way a port misses behavioral coverage.

Expected-failing test ports should live under a `known_failures` test module and use `pacquet_testing_utils::allow_known_failure!` at the not-yet-implemented subject-under-test boundary. List all expected failures with `just known-failures`.

Test the tests before marking them ported. After porting a test, temporarily modify the relevant implementation path so the test should fail, run that test, and verify it fails for the expected reason. Revert the temporary breakage before committing. This guards against porting tests that execute but do not actually detect the behavior they claim to cover. See https://github.com/pnpm/pacquet/issues/299#issuecomment-4323032648.

**Revert the breakage with `git restore <file>` (or by editing the file), never by moving a saved backup copy into place.** Cargo's freshness check is mtime-based: a restore that carries the backup's old mtime (`mv`, `cp -p`, Python's `shutil.move`, …) leaves the artifact compiled from the *broken* source looking newer than the source, so every later `cargo test` / `cargo nextest` run keeps executing the broken binary. The resulting failures look flaky — unrelated tests fail with impossible states, single runs and full runs disagree — and nothing points back at the stale artifact. `git restore` writes the file fresh (current mtime) and triggers the rebuild; if you must restore some other way, follow it with `touch <file>`. When test outcomes ever flip with no code change, suspect a stale artifact first: `touch` the implementation file and rerun.

Having more tests than pnpm is a plus, but it is not strictly required. The lists in this plan are a floor, not a ceiling. Porting the upstream coverage is the minimum bar for behavioral parity. Beyond that minimum, pacquet-only tests that exercise edge cases, regressions, or invariants the upstream suite does not cover are welcome and encouraged, but contributors are not obligated to add them. Do not hold back extra coverage just to keep the two suites symmetric.

## Workspace Lockfile Freshness

Multi-importer parity coverage:

- [x] `TypeScript repo: installing/deps-installer/test/install/frozenLockfile.ts:55` `frozen-lockfile: fail on a shared pnpm-lock.yaml that does not satisfy one of the package.json files` — ported as `changed_registry_specifier_in_workspace_importer_invalidates_lockfile` in `crates/cli/tests/workspace_install.rs`.
- [x] `TypeScript repo: lockfile/verification/test/allProjectsAreUpToDate.test.ts:28` `works with packages linked through the workspace protocol using relative path` — covered end-to-end by `shared_workspace_dep_link_is_relative_to_each_importer`.
- [x] `TypeScript repo: lockfile/verification/test/allProjectsAreUpToDate.test.ts:70` `works with aliased local dependencies` — ported as `returns_up_to_date_when_aliased_workspace_dependency_satisfies_range`.
- [x] `TypeScript repo: lockfile/verification/test/allProjectsAreUpToDate.test.ts:112` `works with aliased local dependencies that specify versions` — covered by the same alias-range test.
- [x] `TypeScript repo: lockfile/verification/test/allProjectsAreUpToDate.test.ts:154` `returns false if the aliased dependency version is out of date` — ported as `returns_skipped_when_aliased_workspace_dependency_version_is_outdated`.
- [x] `TypeScript repo: lockfile/verification/test/allProjectsAreUpToDate.test.ts:196` `use link and registry version if linkWorkspacePackages = false` — ported as `returns_up_to_date_for_registry_resolution_when_workspace_linking_is_off`.
- [x] `TypeScript repo: lockfile/verification/test/allProjectsAreUpToDate.test.ts:288` `returns false if dependenciesMeta differs` — covered end-to-end by `workspace_importer_dependencies_meta_is_checked`.
- [x] `TypeScript repo: lockfile/verification/test/allProjectsAreUpToDate.test.ts:335` `returns true if dependenciesMeta matches` — the same integration test first proves a populated matching map succeeds under `--frozen-lockfile`, then removes it and proves the mismatch fails.
- [x] `TypeScript repo: lockfile/verification/test/allProjectsAreUpToDate.test.ts:702` `returns true if workspace dependency's version type is tag` — ported as `returns_up_to_date_when_linked_workspace_dependency_uses_a_tag`.
- [x] `TypeScript repo: lockfile/verification/test/allProjectsAreUpToDate.test.ts:749` `returns false if one of the importers is not present in the lockfile` — covered end-to-end by `missing_workspace_importer_is_not_accepted_by_frozen_install`.
- [x] `TypeScript repo: lockfile/verification/test/allProjectsAreUpToDate.test.ts:812` `returns true if one of the importers is not present in the lockfile but the importer has no dependencies` — covered by `normal_install_accepts_missing_dependency_free_workspace_importer` and the effective-manifest variant `normal_install_accepts_missing_importer_with_only_ignored_optional_dependencies`; the explicit frozen path remains strict, matching `frozenLockfile.ts`.
- [x] `TypeScript repo: lockfile/verification/test/allProjectsAreUpToDate.test.ts:872` `returns true for injected self-referencing file: dependency resolved as link:` — ported as `injected_self_reference_resolved_as_link_is_up_to_date`.
- [x] `TypeScript repo: lockfile/verification/test/allProjectsAreUpToDate.test.ts:918` `returns false if the lockfile is broken, the resolved versions do not satisfy the ranges` — ported as `resolved_version_outside_manifest_range_is_stale` in `crates/lockfile/src/freshness/tests.rs`.

Pacquet also keeps the issue-specific add-and-recover flow in `changed_workspace_importer_invalidates_lockfile`: adding a `workspace:*` dependency to a member makes a frozen install fail and a normal install refresh and link it.

### `satisfiesPackageManifest` unit-level coverage

`crates/lockfile/src/freshness.rs::satisfies_package_manifest` ports `lockfile/verification/src/satisfiesPackageManifest.ts`; unit tests live in `crates/lockfile/src/freshness/tests.rs`.

- [x] `TypeScript repo: lockfile/verification/test/satisfiesPackageManifest.ts:255` / `:278` `autoInstallPeers` — peers auto-installed into the importer's `dependencies` (pnpm's default) must not read as drift — ported as `peer_only_dependency_is_satisfied_when_auto_install_peers`, `peers_also_declared_as_regular_deps_still_satisfy`, plus `frozen_install_accepts_auto_installed_workspace_peer` in `crates/cli/tests/workspace_install.rs`. Pacquet-only `peer_only_dependency_is_stale_without_auto_install_peers` pins the `auto_install_peers = false` branch.
- [x] `TypeScript repo: lockfile/verification/test/satisfiesPackageManifest.ts:55` optional-only manifest vs prod-only lockfile → stale — ported as `manifest_optional_only_but_lockfile_records_prod_is_stale`.
- [x] dev-only / optional-only field matches — `dev_only_dependency_match_satisfies`, `optional_only_dependency_match_satisfies`.
- [x] `TypeScript repo: lockfile/verification/test/satisfiesPackageManifest.ts:362` `excludeLinksFromLockfile: true` drops `link:`-protocol deps from both the flat diff and the per-field check — `exclude_linked_dependencies_drops_link_deps_from_every_group` pins the manifest normalization used by the freshness check.

The v6/v7-shape scenarios (a top-level `importer.specifiers` map diverging from the dependency fields, `satisfiesPackageManifest.ts:103` / `:169`) are not representable in pacquet's inline-specifier v9 model; the equivalent v9 drift is covered by `manifest_adds_dep_returns_specifier_diff` / `manifest_drops_dep_returns_specifier_diff`. The `no importer` case (`:203`) is enforced at the caller (`check_importer_satisfies`) and covered by `missing_workspace_importer_is_not_accepted_by_frozen_install`.

## `.modules.yaml` Write And Verify

Primary tests:

- [x] `TypeScript repo: installing/modules-yaml/test/index.ts:10` `writeModulesManifest() and readModulesManifest()`
- [x] `TypeScript repo: installing/modules-yaml/test/index.ts:42` `backward compatible read of .modules.yaml created with shamefully-hoist=true`
- [x] `TypeScript repo: installing/modules-yaml/test/index.ts:55` `backward compatible read of .modules.yaml created with shamefully-hoist=false`
- [x] `TypeScript repo: installing/modules-yaml/test/index.ts:68` `readModulesManifest() should create a node_modules directory`
- [x] `TypeScript repo: installing/modules-yaml/test/index.ts:96` `readModulesManifest does not fail on empty file`

Frozen/headless install coverage:

- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:54` `installing a simple project` verifies headless install writes a modules manifest — ported as `frozen_reinstall_writes_modules_manifest_current_lockfile_and_bins` in `crates/cli/tests/install_state.rs`.
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:819` `installing with no symlinks but with PnP` verifies `.modules.yaml` still exists when symlinks are disabled — ported as `pnp_install_without_symlinks_still_writes_modules_manifest_and_bin_directory` in `crates/cli/tests/install_state.rs`, including `.pnp.cjs` resolution and the no-importer-symlink layout.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:24` `should hoist dependencies` verifies `hoistedDependencies` is preserved on repeat frozen install — ported as `should_hoist_dependencies_repeat_install_preserves_map` in `crates/cli/tests/hoist.rs` (re-resolving repeat install and frozen re-materialization both reproduce the map byte-for-byte).
- [x] `TypeScript repo: installing/deps-installer/test/install/modulesCache.ts:52` `the modules cache is pruned when it expires and headless install is used` verifies `prunedAt` is read, rewritten, and honored by headless install — ported as `expired_modules_cache_is_pruned_during_frozen_install` in `crates/cli/tests/install_state.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:74` `skip optional dependency that does not support the current OS` verifies `skipped` survives frozen reinstall — ported as `skip_optional_dependency_that_does_not_support_the_current_os` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/lockfile.ts:614` `pendingBuilds gets updated if install removes packages` verifies `.modules.yaml.pendingBuilds` is rewritten after pruning — ported as `pending_builds::removing_a_package_shrinks_the_list` in `crates/cli/tests/lifecycle_scripts.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:205` `GVS re-links when allowBuilds changes` verifies GVS-related `allowBuilds` state is updated in `.modules.yaml` — ported as `gvs_relinks_when_allow_builds_changes` in `crates/cli/tests/global_virtual_store.rs`, which also drove populating `Modules::allow_builds` (previously always unset). Adjacent non-GVS coverage is `rebuild_after_allow_builds_changes` in `crates/cli/tests/lifecycle_scripts.rs`.
- [ ] `TypeScript repo: pnpm/test/monorepo/index.ts:1467` `custom virtual store directory in a workspace with not shared lockfile` verifies frozen reinstall preserves custom `virtualStoreDir` serialization — stubbed in `known_failures::custom_virtual_store_directory_with_dedicated_lockfiles` (`crates/cli/tests/multiple_importers.rs`; needs `sharedWorkspaceLockfile: false`).
- [x] `TypeScript repo: pnpm/test/monorepo/index.ts:1514` `custom virtual store directory in a workspace with shared lockfile` verifies frozen reinstall preserves root `virtualStoreDir` serialization — ported as `custom_virtual_store_directory_in_a_workspace_with_shared_lockfile` in `crates/cli/tests/multiple_importers.rs`.

Rust port notes:

- Start with pure read/write tests before install tests.
- Copy the TypeScript legacy fixtures from `TypeScript repo: installing/modules-yaml/test/fixtures/`.
- Assert raw serialized `virtualStoreDir` where the TS test checks serialization, not normalized path resolution.

## Proper Support Of `optionalDependencies`

Primary frozen/headless tests:

- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:74` `skip optional dependency that does not support the current OS` removes `node_modules`, reinstalls with `frozenLockfile: true`, and verifies skipped packages remain skipped — ported as `skip_optional_dependency_that_does_not_support_the_current_os` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:283` `optional subdependency is skipped` includes forced headless install with `force: true, frozenLockfile: true` and verifies incompatible optional subdependency handling — ported as `optional_subdependency_is_skipped` in `crates/cli/tests/optional_dependencies.rs`; the forced-headless tail is `forced_frozen_install_materializes_incompatible_optionals` in the same file.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:359` `only that package is skipped which is an optional dependency only and not installable` removes `node_modules`, reinstalls frozen, and guards optional/non-optional overlap — ported as `only_optional_only_and_not_installable_package_is_skipped` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:594` `install optional dependency for the supported architecture set by the user (nodeLinker=%s)` includes `nodeLinker` variants and frozen reinstall — ported as `install_optional_dependency_for_the_supported_architectures` in `crates/cli/tests/optional_dependencies.rs` (both linkers in one test; isolated-variant resolution is asserted through the `.pnpm/node_modules` fallback, matching upstream's `deepRequireCwd`).
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:665` `optional dependency is hardlinked to the store if it does not require a build` includes frozen reinstall and import-method parity — ported (Unix) as `optional_dependency_is_hardlinked_to_the_store_if_it_does_not_require_a_build` in `crates/cli/tests/optional_dependencies.rs`, asserting the on-disk inode sharing instead of the `pnpm:progress` emission.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:540` `hoisting should not create a broken symlink to a skipped optional dependency` covers public hoist plus skipped optional dependency in headless behavior — ported as `hoisting_skips_broken_symlink_for_skipped_optional` in `crates/cli/tests/hoist.rs` (previously a `known_failures` stub).

Supporting tests:

- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:300` `installing only optional deps` covers headless include filtering when only optional dependencies are selected — ported as `headless_install_include_filtering_excludes_production_group` in `crates/cli/tests/optional_dependencies.rs` via the CLI-expressible `--dev` include set (upstream's dependencies-and-dev-both-false set exists only in the programmatic API).
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:323` `not installing optional deps` covers headless include filtering — ported as `headless_install_without_optional_deps` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:340` `skipping optional dependency if it cannot be fetched` verifies a failed optional fetch does not fail headless install and still writes install state — ported as `headless_install_skips_unfetchable_optional_dependency` in `crates/cli/tests/optional_dependencies.rs` (integrity corruption stands in for upstream's unresolvable-tarball fixture).
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:21` `successfully install optional dependency with subdependencies` — ported as `install_optional_dependency_with_subdependencies` in `crates/cli/tests/optional_dependencies.rs` (registry-mock fixture stands in for `fsevents`).
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:27` `skip failing optional dependencies` — ported as `skip_failing_optional_dependencies` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:34` `skip failing optional peer dependencies` — ported as `skip_failing_optional_peer_dependencies` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:45` `skip non-existing optional dependency` — ported as `skip_non_existing_optional_dependency` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:143` `skip optional dependency that does not support the current Node version` — ported as `skip_optional_dependency_that_does_not_support_the_current_node_version` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:169` `do not skip optional dependency that does not support the current pnpm version` — ported as `do_not_skip_optional_dependency_that_does_not_support_the_current_pnpm_version` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:199` `don't skip optional dependency that does not support the current OS when forcing` — ported as `do_not_skip_unsupported_os_optional_dependency_when_forcing` in `crates/cli/tests/optional_dependencies.rs` (`install --force` landed, pnpm/pnpm#13142).
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:213` `optional subdependency is not removed from current lockfile when new dependency added` — ported as `optional_subdependency_stays_in_current_lockfile_when_new_dependency_added` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:344` `optional subdependency of newly added optional dependency is skipped` — ported as `optional_subdependency_of_newly_added_optional_dependency_is_skipped` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:391` `not installing optional dependencies when optional is false` — ported as `not_installing_optional_dependencies_when_optional_is_false` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:419` `optional dependency has bigger priority than regular dependency` — ported as `optional_dependency_has_bigger_priority_than_regular_dependency` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:436` `only skip optional dependencies` — ported as `only_optional_dependencies_are_skipped_in_a_mixed_graph` with registry fixtures that share a required subtree with an incompatible optional root.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:470` `skip optional dependency that does not support the current OS, when doing install on a subset of workspace projects` — ported as the real `skip_unsupported_optional_when_installing_a_workspace_subset` in `crates/cli/tests/optional_dependencies.rs` (previously a `known_failures` stub; `--filter` selected-projects installs landed in pnpm/pnpm#13030).
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:540` `do not fail on unsupported dependency of optional dependency` — ported as `do_not_fail_on_unsupported_dependency_of_optional_dependency` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:552` `fail on unsupported dependency of optional dependency` — ported as `fail_on_unsupported_dependency_of_optional_dependency` in `crates/cli/tests/optional_dependencies.rs` (`engineStrict` now dispatches edge-aware over the lockfile graph, pnpm/pnpm#13143).
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:563` `do not fail on an optional dependency that has a non-optional dependency with a failing postinstall script` — ported as `do_not_fail_on_optional_dependency_with_failing_non_optional_postinstall` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:574` `fail on a package with failing postinstall if the package is both an optional and non-optional dependency` — ported as `fail_on_failing_postinstall_when_package_is_both_optional_and_non_optional` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:618` `remove optional dependencies that are not used` — ported as `remove_optional_dependencies_that_are_not_used` in `crates/cli/tests/optional_dependencies.rs` (the architecture change is driven through `pnpm-workspace.yaml`, which is also what makes the up-to-date fast path re-evaluate).
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:633` `remove optional dependencies that are not used, when hoisted node linker is used` — ported as `remove_optional_dependencies_that_are_not_used_with_hoisted_linker` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:648` `remove optional dependencies if supported architectures have changed and a new dependency is added` — ported as `remove_optional_dependencies_when_architectures_change_and_a_dependency_is_added` in `crates/cli/tests/optional_dependencies.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:703` `complex scenario with same optional dependencies appearing in many places of the dependency graph` — ported as `repeated_optional_dependencies_across_a_complex_graph_are_classified_per_edge` with two selector versions sharing platform-optional packages.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:712` `dependency that is both optional and non-optional is installed, when optional dependencies should be skipped` — ported as `both_optional_and_non_optional_dependency_is_installed_when_optionals_are_skipped` in `crates/cli/tests/optional_dependencies.rs` (registry-mock fixtures stand in for `@babel/cli` + `del`).
- [x] `TypeScript repo: resolving/npm-resolver/test/optionalDependencies.test.ts:27` `optional dependencies receive full metadata with libc field` ensures optional dependency metadata includes platform/libc fields — `optional_opt_forces_full_metadata_endpoint` now asserts the picked full manifest preserves `libc`.
- [x] `TypeScript repo: resolving/npm-resolver/test/optionalDependencies.test.ts:73` `abbreviated and full metadata are cached separately` prevents regular dependency metadata cache from hiding optional metadata — ported as `cache_key_separates_abbreviated_from_full` in `crates/resolving-npm-resolver/src/pick_package/tests.rs`.
- [x] `TypeScript repo: installing/package-requester/test/index.ts:852` `do not fetch an optional package that is not installable` covers cold-store requester behavior for unsupported optional packages — ported as `skips_prefetch_for_unsupported_optional_manifest` (plus its sibling cases) in `crates/package-manager/src/prefetching_resolver/tests.rs`.
- [x] `TypeScript repo: installing/package-requester/test/index.ts:1205` `should pass optional flag to resolve function` ensures resolver receives `optional: true` — ported as `passes_optional_flag_to_the_resolver` in `crates/resolving-deps-resolver/src/tests.rs`.

Rust port notes:

- Separate platform/architecture skip semantics from the generic optional dependency group filtering.
- These tests depend on `.modules.yaml.skipped`, so port that field first.

## Manifest Group Mutations (`add` / `installSome`)

The `installSome` suites in `installing/deps-installer/test/install/updatingPkgJson.ts` and `install/misc.ts` were not previously enumerated here; this list is their audit. They cover how `pnpm add` writes the manifest and how the install that follows treats the other dependency groups.

- [x] `TypeScript repo: installing/deps-installer/test/install/updatingPkgJson.ts:112` `dependency should be removed from the old field when installing it as a different type of dependency` — ported as `add_moves_dependency_to_new_group_and_keeps_other_groups` in `crates/cli/tests/add.rs` (explicit `@^100.0.0` selectors stand in for upstream's `latest` dist-tag setup).
- [x] `TypeScript repo: installing/deps-installer/test/install/updatingPkgJson.ts:13` `save to package.json (is-positive@^1.0.0)` — covered by `should_add_to_package_json` and `add_explicit_range_resolves_to_concrete_version` in `crates/cli/tests/add.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/updatingPkgJson.ts:23` `don't override existing spec in package.json on named installation` — covered by `add_existing_dependency_without_version_keeps_tilde_range` / `..._keeps_exact_pin` in `crates/cli/tests/add.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/updatingPkgJson.ts:57` `dependency should not be added to package.json if it is already there` — covered by the same keeps-range tests in `crates/cli/tests/add.rs` (a versionless re-add leaves the declared entry untouched).
- [x] `TypeScript repo: installing/deps-installer/test/install/updatingPkgJson.ts:88` `dependencies should be updated in the fields where they already are` — ported as `add_updates_dependency_in_the_group_it_already_occupies` in `crates/cli/tests/add.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/updatingPkgJson.ts:198` `an update bumps the versions in the manifest` — covered by `update_latest_rewrites_manifest` (and the compatible-update range-preservation tests) in `crates/cli/tests/update.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/misc.ts:527` re-add of a dev dependency at a dist-tag keeps the `devDependencies` home — ported as `readding_a_dev_dependency_at_a_dist_tag_keeps_its_group` in `crates/cli/tests/add.rs`.

## Hoisting (`hoistPattern`, `publicHoistPattern`)

Primary tests:

- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:24` `should hoist dependencies` repeats install with `hoistPattern: '*'` and `frozenLockfile: true`. Single-importer subset ported as `private_hoist_default_pattern_hoists_transitives` in `crates/cli/tests/hoist.rs`; the repeat-install map preservation is `should_hoist_dependencies_repeat_install_preserves_map`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:53` `should hoist dependencies to the root of node_modules when publicHoistPattern is used` covers baseline public hoist behavior. Ported as `public_hoist_star_hoists_to_root_node_modules`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:71` `public hoist should not override directories that are already in the root of node_modules` — ported as `public_hoist_does_not_override_an_existing_root_directory` in `crates/cli/tests/hoist.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:89` `should hoist some dependencies to the root of node_modules when publicHoistPattern is used and others to the virtual store directory` covers combined private and public hoist patterns. Ported as `combined_public_and_private_hoist_patterns_split_targets` in `crates/cli/tests/hoist.rs` (registry-mock fixtures stand in for the upstream package set).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:107` `should hoist dependencies by pattern` covers pattern-specific private hoisting. Ported as `private_hoist_pattern_filters_aliases`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:121` `should remove hoisted dependencies`. Ported as `should_remove_hoisted_dependencies` in `crates/cli/tests/hoist.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:137` `should not override root packages with hoisted dependencies`. Ported as `should_not_override_root_packages_with_hoisted_deps`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:148` `should rehoist when uninstalling a package`. Ported as `should_rehoist_when_uninstalling_a_package`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:169` `should rehoist after running a general install`. Ported as `should_rehoist_after_running_a_general_install`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:201` `should not override aliased dependencies`. Ported as `should_not_override_aliased_dependencies`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:209` `hoistPattern=* throws exception when executed on node_modules installed w/o the option`. Ported as `hoist_pattern_mismatch_throws_against_existing_modules_yaml` (`ERR_PNPM_HOIST_PATTERN_DIFF` on `add` against a drifted layout).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:220` `hoistPattern=undefined throws exception when executed on node_modules installed with hoist-pattern=*`. Ported as `hoist_pattern_undefined_throws_against_hoisted_modules_yaml`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:233` `hoist by alias`. Ported as `hoist_by_alias` in `crates/cli/tests/hoist.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:249` `should remove aliased hoisted dependencies`. Ported as `should_remove_aliased_hoisted_dependencies`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:272` `should update .modules.yaml when pruning if we are flattening`. Ported as `modules_yaml_updated_on_prune_when_flattening`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:288` `should rehoist after pruning`. Ported as `should_rehoist_after_pruning`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:320` `should hoist correctly peer dependencies`. Ported as the real `should_hoist_correctly_peer_dependencies` in `crates/cli/tests/hoist.rs` (local `ajv`/`ajv-keywords` fixtures added to the registry mock; previously a `known_failures` stub).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:327` `should uninstall correctly peer dependencies`. Ported as the real `should_uninstall_correctly_peer_dependencies` (drops the dep from the manifest, regenerates the lockfile, and replays a frozen install standing in for upstream's `uninstallSome` mutation).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:341` `hoist-pattern: hoist all dependencies to the virtual store node_modules` covers workspace install followed by frozen reinstall. Ported as `workspace_hoist_walks_every_importer` (fresh half) plus `workspace_hoist_all_to_virtual_store_node_modules` in `crates/cli/tests/hoist.rs` (rimraf-then-frozen-replay reproduces the exact hoist layout).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:423` `hoist when updating in one of the workspace projects`. Ported as `workspace_hoist_when_updating_one_project` in `crates/cli/tests/hoist.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:514` `should recreate node_modules with hoisting`. Ported as `should_recreate_node_modules_with_hoisting`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:540` `hoisting should not create a broken symlink to a skipped optional dependency` covers hoisting with skipped optional packages. Ported as the real `hoisting_skips_broken_symlink_for_skipped_optional` in `crates/cli/tests/hoist.rs` (previously a `known_failures` stub).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:567` `the hoisted packages should not override the bin files of the direct dependencies` covers public hoist bin precedence after frozen reinstall. Ported as `hoisted_packages_dont_override_direct_dep_bins` in `crates/cli/tests/hoist.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:587` `hoist packages which is in the dependencies tree of the selected projects`. Ported as the real `workspace_hoist_packages_in_selected_projects_tree` in `crates/cli/tests/hoist.rs` (previously a `known_failures` stub — `--filter` selected-projects installs landed in pnpm/pnpm#13030).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:682` `only hoist packages which is in the dependencies tree of the selected projects with sub dependencies`. Ported as the real `workspace_hoist_only_in_selected_projects_with_subdeps` in `crates/cli/tests/hoist.rs` (the divergent per-parent subdependency pins are produced by repinning the generated lockfile, standing in for upstream's hand-written one).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:790` `should add extra node paths to command shims` — ported as `should_add_extra_node_paths_to_command_shims` in `crates/cli/tests/hoist.rs` (`extendNodePath` landed).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:799` `should not add extra node paths to command shims, when extend-node-path is set to false` — ported as `should_not_add_extra_node_paths_when_extend_node_path_false` in `crates/cli/tests/hoist.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:813` `hoistWorkspacePackages should hoist all workspace projects` covers workspace package hoisting and frozen reinstall. Ported as `hoist_workspace_packages_links_projects_by_name` in `crates/cli/tests/hoist.rs` (loops `hoistWorkspacePackages` on/off, asserts the name-links into `.pnpm/node_modules`, and replays `--frozen-lockfile` after deleting the root `node_modules` to cover the preservation tail).

Headless module-manifest checks:

- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:569` `installing with hoistPattern=*` asserts private `hoistedDependencies` in `.modules.yaml`. Ported as `modules_yaml_records_hoisted_dependencies` and `private_hoist_links_bins` in `crates/cli/tests/hoist.rs`.
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:628` `installing with publicHoistPattern=*` asserts public `hoistedDependencies` in `.modules.yaml`. Ported as `public_hoist_star_hoists_to_root_node_modules` and `public_hoist_bin_is_linked_via_root_bin_dir`.
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:690` `installing with publicHoistPattern=* in a project with external lockfile` covers headless public hoist with an external lockfile/project root split — ported as `public_hoist_uses_the_project_root_when_the_lockfile_is_external` in `crates/cli/tests/install_state.rs`.
- [ ] `TypeScript repo: installing/deps-installer/test/install/sideEffects.ts:50` `caching side effects of native package when hoisting is used` is skipped upstream but documents side-effects cache behavior under private hoisting.

Rust port notes:

- Port the module-manifest assertions with hoisting; otherwise later behavior can appear to work while install state is wrong.

## Support `patchedDependencies`

Primary frozen/headless tests:

The four scenarios below share `assert_patch_install_scenario` in `crates/cli/tests/patch.rs`, which ports the whole upstream shape: patched file on disk, lockfile `patchedDependencies` entry and `(patch_hash=…)` snapshot key, the `;patch=<hash>` side-effects-cache row, the frozen reinstall, the frozen *hoisted* reinstall, and the offline unpatched-sibling project. Like upstream it pins `packageImportMethod: hardlink`, without which the sibling check cannot catch a patch that corrupts the shared store copy.

- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:24` `patch package with exact version` — `install_level_exact_version_patch_applies_with_frozen_reinstall`.
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:120` `patch package with version range` — `install_level_range_patch_applies_with_frozen_reinstall`.
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:297` `patch package when scripts are ignored` — `install_level_patch_applies_when_scripts_are_ignored`.
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:386` `patch package when the package is not in allowBuilds list` — `install_level_patch_applies_when_the_package_is_not_in_allow_builds`.

Supporting tests:

- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:848` `installing with no modules directory and a patched dependency` — ported as `installing_with_no_modules_directory_and_a_patched_dependency` in `crates/cli/tests/patch.rs`: `enableModulesDir: false` is now honored from `pnpm-workspace.yaml` (it rides the lockfile-only pipeline in `Install::run`; the NAPI binding keeps its own aliasing in `crates/napi/src/install.rs`).
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:216` `patch package reports warning if not all patches are applied and allowUnusedPatches is set`
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:246` `patch package throws an exception if not all patches are applied`
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:269` `the patched package is updated if the patch is modified` — `install_level_modified_patch_is_reapplied`.
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:475` `patch package when the patched package has no dependencies and appears multiple times` — `install_level_patch_applies_to_a_package_reached_multiple_times`.
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:508` `patch package should fail when the exact version patch fails to apply` — `install_level_exact_version_patch_that_does_not_apply_fails`.
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:530` `patch package should fail when the version range patch fails to apply` — `install_level_range_patch_that_does_not_apply_fails`.
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:552` `patch package should fail when the name-only range patch fails to apply` — `install_level_name_only_patch_that_does_not_apply_fails`.

Rust port notes:

- Ported `allowUnusedPatches` warning test and `ERR_PNPM_UNUSED_PATCH` error test in `crates/cli/tests/patch.rs` as part of the `allow_unused_patches` config wiring.
- The three install-level apply-failure variants assert `ERR_PNPM_PATCH_FAILED` plus upstream's `Could not apply patch` prefix. Unit-level coverage stays in `crates/patching/src/apply/tests.rs` (`unmatching_hunk_errors_patch_failed`, `missing_target_file_errors_patch_failed`).
- The hoisted ports surfaced a pacquet-only bug: `BuildModules` resolved one `pkgRoot` per snapshot, so a package the hoisted walker nests under several consumers (version conflict) was patched at only one of them, and a warm reinstall re-imported the cached overlay at only one of them. `pkg_roots_by_key` now carries every location; the head still runs scripts and seeds the cache, while patch application and overlay re-imports walk the list. Pinned by `hoisted_patch_reaches_every_nested_copy_of_a_package`.

## Support Building Dependencies

Primary frozen/headless tests:

- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:331` `lifecycle scripts run before linking bins` removes `node_modules`, reinstalls frozen, and verifies generated bins are executable — ported as `lifecycle_scripts_run_before_linking_bins` in `crates/cli/tests/lifecycle_scripts.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:351` `hoisting does not fail on commands that will be created by lifecycle scripts on a later stage` covers `hoistPattern: '*'` and frozen install — ported as `hoisting_tolerates_bins_created_by_a_later_lifecycle_stage` in `crates/cli/tests/lifecycle_scripts.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:372` `bins are linked even if lifecycle scripts are ignored` — ported as `bins_linked_even_if_scripts_ignored` in `crates/cli/tests/lifecycle_scripts.rs`, including the frozen-reinstall tail.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:408` `dependency should not be added to current lockfile if it was not built successfully during headless install` covers failed build during frozen/headless install — ported as `a_failed_build_writes_no_current_lockfile` in `crates/cli/tests/current_lockfile.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:445` `selectively ignore scripts in some dependencies by allowBuilds (not others)` — ported as `selectively_ignore_scripts_by_allow_builds` in `crates/cli/tests/lifecycle_scripts.rs`, including the frozen-reinstall tail.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:466` `selectively allow scripts in some dependencies by allowBuilds` — ported as `selectively_allow_scripts_by_allow_builds` in `crates/cli/tests/lifecycle_scripts.rs` (frozen reinstall and the `pnpm:ignored-scripts` reporting assertions included).
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:504` `selectively allow scripts in some dependencies by allowBuilds using exact versions` covers exact-version allow list — ported as `selectively_allow_scripts_by_allow_builds_exact_versions` in `crates/cli/tests/lifecycle_scripts.rs` (frozen reinstall and the `pnpm:ignored-scripts` reporting assertions included).
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:552` `lifecycle scripts run after linking root dependencies` verifies builds can require root dependencies during frozen install — ported as `lifecycle_scripts_run_after_linking_root_deps` in `crates/cli/tests/lifecycle_scripts.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:724` `build dependencies that were not previously built after allowBuilds changes` covers rebuilding newly allowed dependencies with frozen install — ported as `rebuild_after_allow_builds_changes` in `crates/cli/tests/lifecycle_scripts.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:1902` `link the bin file of a workspace project that is created by a lifecycle script` covers workspace build-created bin behavior and frozen reinstall. Ported as `link_bin_of_workspace_project_created_by_lifecycle_script` in `crates/cli/tests/multiple_importers.rs`.

Supporting tests:

- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:362` `run pre/postinstall scripts` verifies headless build execution and `pendingBuilds` when scripts are ignored — the script-execution half is `headless_run_pre_postinstall_scripts`; the `ignoreScripts` tail is `pending_builds::ignore_scripts_records_the_project_and_its_deferred_dependency`, both in `crates/cli/tests/lifecycle_scripts.rs`.
- [x] `TypeScript repo: pnpm/test/install/lifecycleScripts.ts:245` `the list of ignored builds is preserved after a repeat install` covers CLI-level `.modules.yaml.ignoredBuilds` persistence — ported as `ignored_builds_are_preserved_after_a_repeat_install` in `crates/cli/tests/lifecycle_scripts.rs`.
- [x] `TypeScript repo: pnpm/test/install/lifecycleScripts.ts:303` `strictDepBuilds fails for packages with cached side-effects (#11035)` ensures cached side effects do not bypass build approval — ported as `strict_dep_builds_fails_for_packages_with_cached_side_effects` in `crates/cli/tests/lifecycle_scripts.rs`. The port found the frozen no-op fast path exiting 0 on a withdrawn approval; `.modules.yaml` now records `allowBuilds` and `has_revoked_allowed_builds` sends that case to the full install.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:26` `run pre/postinstall scripts` — ported as `run_pre_and_postinstall_scripts` in `crates/cli/tests/lifecycle_scripts.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:60` `return the list of packages that should be build` — the N-API install result collects deduplicated `pnpm:ignored-scripts` names as `depsRequiringBuild`, pinned by `ignored_scripts_are_returned_as_dependencies_requiring_build` in `crates/napi/src/reporter_bridge.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:121` `run install scripts` — ported as `run_install_scripts` in `crates/cli/tests/lifecycle_scripts.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:175` `installation fails if lifecycle script fails` — ported at unit level as `fail_when_failing_postinstall_is_required` in `crates/package-manager/src/build_modules/tests.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:303` `run lifecycle scripts of dependent packages after running scripts of their deps` — ported as `lifecycle_scripts_run_in_dependency_order` in `crates/cli/tests/lifecycle_scripts.rs`.

Rust port notes:

- Port the frozen/headless tests before broad lifecycle coverage.
- Current-lockfile behavior on failed build should be paired with the current-lockfile TODO.

## Support Side-Effects Cache

Primary side-effects tests:

- [x] `TypeScript repo: installing/deps-installer/test/install/sideEffects.ts:79` `using side effects cache` covers side-effects read/write — ported at unit level as `using_side_effects_cache_skips_rebuild` (read) and `write_path_populates_side_effects_row` (write) in `crates/package-manager/src/build_modules/tests.rs`, plus the end-to-end `side_effects_materialized_on_warm_frozen_reinstall` in `crates/cli/tests/side_effects_cache.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/sideEffects.ts:166` `uploading errors do not interrupt installation` verifies cache upload errors do not fail install — ported as `upload_error_does_not_interrupt_install` in `crates/package-manager/src/build_modules/tests.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/sideEffects.ts:189` `a postinstall script does not modify the original sources added to the store` verifies side effects stay separate from original CAFS files — covered by `write_path_populates_side_effects_row` in `crates/package-manager/src/build_modules/tests.rs` (drives a source-modifying postinstall and asserts the originals stay intact).
- [x] `TypeScript repo: installing/deps-installer/test/install/sideEffects.ts:225` `a corrupted side-effects cache is ignored` verifies fallback when cache contents are invalid — ported as `corrupt_side_effects_cache_falls_back_to_rebuild` in `crates/package-manager/src/build_modules/tests.rs`.

Frozen/headless cross-coverage:

- [ ] `TypeScript repo: installing/deps-installer/test/install/sideEffects.ts:50` `caching side effects of native package when hoisting is used` is skipped upstream but relevant to hoisting plus side-effects cache.
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:24` `patch package with exact version` — the side-effects-cache half is ported with the scenario (see Support `patchedDependencies`): `assert_patched_side_effects_cached` pins the `;patch=<hash>` row and that the cached `index.js` digest differs from the pristine one.
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:120` `patch package with version range` — same coverage as the exact-version entry above.
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:297` `patch package when scripts are ignored` — same, and pins that `--ignore-scripts` still produces a patched side-effects row (the key drops its `;deps=` segment but keeps `;patch=`).
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:386` `patch package when the package is not in allowBuilds list` — same coverage.
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:706` `using side effects cache with nodeLinker=%s` covers headless side-effects behavior for isolated and hoisted linkers — ported as `side_effects_materialized_on_warm_frozen_reinstall` and `side_effects_materialized_on_warm_frozen_reinstall_with_hoisted_linker` in `crates/cli/tests/side_effects_cache.rs`.
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:761` `using side effects cache and hoistPattern=*` is skipped upstream but documents intended headless plus hoisting coverage.
- [x] `TypeScript repo: pnpm/test/install/lifecycleScripts.ts:303` `strictDepBuilds fails for packages with cached side-effects (#11035)` verifies build approval semantics even when side effects are cached — ported as `strict_dep_builds_fails_for_packages_with_cached_side_effects` in `crates/cli/tests/lifecycle_scripts.rs`.

Rust port notes:

- Do not block side-effects cache porting on patches; port the standalone `sideEffects.ts` tests first.
- The patch tests become important once patched builds and side-effects cache both exist.

## Support Workspaces

Primary frozen/headless tests:

- [x] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:438` `dependencies of other importers are not pruned when (headless) installing for a subset of importers` — ported as `deps_of_other_importers_are_not_pruned_when_headless_installing_a_subset` in `crates/cli/tests/multiple_importers.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:208` `install only the dependencies of the specified importer. The current lockfile has importers that do not exist anymore` — ported as `stale_current_lockfile_importers_are_retained_on_subset_install` in `crates/cli/tests/multiple_importers.rs` (the project leaves the workspace on disk; upstream shrinks the mutated-importer set instead).
- [x] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:730` `current lockfile contains only installed dependencies when adding a new importer to workspace with shared lockfile` — ported as `current_lockfile_contains_only_installed_dependencies` in `crates/cli/tests/multiple_importers.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:540` `headless install is used when package linked to another package in the workspace` — ported as `headless_install_is_used_when_package_is_linked_to_another_workspace_package` in `crates/cli/tests/multiple_importers.rs`. The upstream tail (the unselected link target's own deps stay uninstalled) is stubbed in `known_failures::subset_install_does_not_install_unselected_link_targets_dependencies`: pacquet's subset closure deep-installs importer-level link targets, upstream keeps them shallow — a cross-stack decision is needed.
- [x] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:598` `headless install is used with an up-to-date lockfile when package references another package via workspace: protocol` — ported as `headless_install_is_used_with_workspace_protocol_references` in `crates/cli/tests/multiple_importers.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:656` `headless install is used when packages are not linked from the workspace (unless workspace ranges are used)` — ported as `headless_install_is_used_when_packages_are_not_linked_from_the_workspace` in `crates/cli/tests/multiple_importers.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:865` `partial installation in a monorepo does not remove dependencies of other workspace projects when lockfile is frozen` — ported as `partial_frozen_install_does_not_remove_dependencies_of_other_workspace_projects` in `crates/cli/tests/multiple_importers.rs` (the divergent transitive pin is produced by repinning the generated lockfile rather than hand-writing one).
- [x] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:1427` `resolve a subdependency from the workspace` — ported as `resolve_a_subdependency_from_the_workspace` in `crates/cli/tests/multiple_importers.rs` (snapshot `link:` + frozen reinstall).
- [x] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:1563` `resolve a subdependency from the workspace, when it uses the workspace protocol` — ported as `resolve_a_subdependency_from_the_workspace_via_workspace_protocol_override` in `crates/cli/tests/multiple_importers.rs` (the `workspace:*` pin arrives through `overrides`).
- [x] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:1768` `symlink local package from the location described in its publishConfig.directory when linkDirectory is true` — ported as `symlink_local_package_from_publish_config_directory` in `crates/cli/tests/multiple_importers.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:1902` `link the bin file of a workspace project that is created by a lifecycle script` — ported as `link_bin_of_workspace_project_created_by_lifecycle_script` in `crates/cli/tests/multiple_importers.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:341` `hoist-pattern: hoist all dependencies to the virtual store node_modules` covers workspace hoisting and frozen reinstall — ported as `workspace_hoist_walks_every_importer` plus the frozen-replay `workspace_hoist_all_to_virtual_store_node_modules` (see the Hoisting section).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:813` `hoistWorkspacePackages should hoist all workspace projects` covers workspace package hoisting and frozen reinstall — ported as `hoist_workspace_packages_links_projects_by_name` in `crates/cli/tests/hoist.rs` (including the frozen-reinstall preservation tail; see the Hoisting section).

Headless restorer tests:

- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:789` `installing in a workspace` — ported as `subset_headless_install_keeps_other_projects_packages_in_current_lockfile` in `crates/cli/tests/multiple_importers.rs`.
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:873` `installing in a workspace with node-linker=hoisted` — ported as `installing_in_a_workspace_with_hoisted_node_linker_frozen` in `crates/cli/tests/hoisted_node_linker.rs`.
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:897` `installing a package deeply installs all required dependencies` — ported as `subset_headless_install_deeply_materializes_workspace_linked_dependencies` in `crates/cli/tests/multiple_importers.rs`. The snapshot-level `link:` is rewritten to upstream's lockfile-relative shape before the frozen install: pacquet's resolver records a registry manifest's `link:` dep relative to the dependent importer, upstream's fixture records it relative to the lockfile dir.

CLI-level frozen workspace tests:

- [x] `TypeScript repo: pnpm/test/monorepo/index.ts:734` `recursive install with shared-workspace-lockfile builds workspace projects in correct order` — ported as `recursive_install_builds_workspace_projects_in_correct_order` in `crates/cli/tests/multiple_importers.rs`.
- [x] `TypeScript repo: pnpm/test/monorepo/index.ts:1281` `dependencies of workspace projects are built during headless installation` — stubbed in `known_failures::workspace_project_dependencies_built_during_headless_install_with_dedicated_lockfiles` (`crates/cli/tests/multiple_importers.rs`): the upstream fixture requires `sharedWorkspaceLockfile: false`, which pacquet's install family rejects.
- [x] `TypeScript repo: pnpm/test/monorepo/index.ts:1317` `linking the package's bin to another workspace package in a monorepo` — ported as `links_workspace_package_bin_into_dependent_project` in `crates/cli/tests/multiple_importers.rs`.
- [x] `TypeScript repo: pnpm/test/monorepo/index.ts:1467` `custom virtual store directory in a workspace with not shared lockfile` — stubbed in `known_failures::custom_virtual_store_directory_with_dedicated_lockfiles` (`crates/cli/tests/multiple_importers.rs`): needs `sharedWorkspaceLockfile: false`.
- [x] `TypeScript repo: pnpm/test/monorepo/index.ts:1514` `custom virtual store directory in a workspace with shared lockfile` — ported as `custom_virtual_store_directory_in_a_workspace_with_shared_lockfile` in `crates/cli/tests/multiple_importers.rs`.

Rust port notes:

- Subset (`--filter`) install tests live in `crates/cli/tests/multiple_importers.rs` and drive the CLI selection that upstream expresses through `mutateModules` subsets.
- Pacquet's subset closure deep-installs importer-level link targets where upstream keeps them shallow (only `--filter <project>...` widens the selection upstream); the divergence is pinned by the `known_failures` stub on the `multipleImporters.ts:540` tail and needs a cross-stack decision.

## Workspace Script `PATH` (`extraBinPaths`)

- [x] `TypeScript repo: pnpm/test/recursive/run.ts:8` `pnpm recursive run finds bins from the root of the workspace` — the run-related assertions are ported as `recursive_run_finds_workspace_root_bin_on_path` and `recursive_run_prefers_project_bin_over_workspace_root_bin` (`crates/cli/tests/run_recursive.rs`) plus `run_finds_workspace_root_bin_on_path` for the member-dir `pnpm run` step (`crates/cli/tests/run.rs`). The upstream test's `-r install` postinstall and `recursive rebuild` steps are install/rebuild coverage, tracked with those features.
- [x] `TypeScript repo: config/reader/test/index.ts:2421` `extraBinPaths` — ported as `extra_bin_paths_lists_workspace_root_bin_only_inside_a_workspace` (`crates/config/src/tests.rs`).

## Workspace Project Filtering (`--filter`)

Ported into the new `pacquet-workspace-projects-filter` and
`pacquet-workspace-projects-graph` crates (the Rust ports of
`@pnpm/workspace.projects-filter` and `@pnpm/workspace.projects-graph`).
The CLI `--filter` / `--filter-prod` flags are parsed into
`Config::filter` / `Config::filter_prod`. Recursive `run` / `exec`
narrow their selected set through these selectors (via
`cli_args::recursive::select_recursive_projects`), and the install
family honors the selection too (pnpm/pnpm#13030): subset installs are
covered end to end in `crates/cli/tests/install_filters.rs` and
`crates/cli/tests/multiple_importers.rs`, and the formerly stubbed
selected-projects hoist / optional-dependency cases are real tests now.

`parseProjectSelector` (ported as `parse_project_selector::tests`):

- [x] `TypeScript repo: workspace/projects-filter/test/parseProjectSelector.ts:198` `parseProjectSelector()` — all 17 fixtures (name, `...`-dependents/dependencies, `^` exclude-self, `./dir`, `{dir}`, `[since]`, and combinations) ported as individual cases.

`filterWorkspaceProjects` (ported as `filter::tests`, fixture mirrors `index.ts:22`):

- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:126` `select only package dependencies (excluding the package itself)`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:138` `select package with dependencies`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:150` `select package with dependencies and dependents, including dependent dependencies`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:163` `select package with dependents`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:175` `select dependents excluding package itself`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:187` `filter using two selectors: one selects dependencies another selects dependents`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:204` `select just a package by name`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:215` `select package without specifying its scope`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:238` `when a scoped package with the same name exists, only pick the exact match`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:271` `when two scoped packages match the searched name, don't select any`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:304` `select by parentDir`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:315` `select by parentDir using glob`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:326` `select by parentDir using globstar`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:337` `select by parentDir with no glob`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:565` `should return unmatched filters`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:577` `select all packages except one`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:591` `select by parentDir and exclude one package by pattern`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:608` `select by parentDir with glob and exclude one package by pattern`.

Changed-packages (`[<since>]`) selectors — git-diff project selection
(`getChangedProjects`) is ported as `get_changed_projects`, and the
tests live in `filter::tests::changed_packages`:

- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:348` `select changed packages`. Ported as `changed_packages::select_changed_packages`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:480` `select changed packages when operating under a git worktree`. Ported as `changed_packages::select_changed_packages_under_git_worktree`.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:553` `selection should fail when diffing to a branch that does not exist`. Ported as `changed_packages::selection_fails_for_nonexistent_diff_branch`.
- [x] `TypeScript repo: pnpm/test/monorepo/index.ts:336` `testPattern is respected by the test script`. Ported as `run_recursive::test_pattern_from_workspace_yaml_is_respected_by_the_test_script`.
- [x] `TypeScript repo: pnpm/test/monorepo/index.ts:404` `changedFilesIgnorePattern is respected`. Ported as `list::changed_files_ignore_pattern_is_respected`.
- [x] `TypeScript repo: config/reader/test/index.ts:2686` `respects testPattern` and `config/reader/test/index.ts:2729` `respects changedFilesIgnorePattern`. Ported as `workspace_yaml::tests::parses_test_pattern_and_changed_files_ignore_pattern_from_yaml_and_applies` (plus the global-config exclusion in `test_pattern_and_changed_files_ignore_pattern_cleared_as_workspace_only_fields`); the upstream `.npmrc`-is-ignored case has no pacquet counterpart because pacquet reads no settings from `.npmrc`.

`createProjectsGraph` has no upstream unit tests (it is exercised only
through `filterProjectsFromDir`'s fixtures upstream); pacquet covers it
with `create_projects_graph::tests` (workspace-spec, version/range,
local-path, strict `linkWorkspacePackages`, and `ignoreDevDeps` edge
resolution).

## Support `nodeLinker=hoisted`

Primary tests:

- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:16` `installing with hoisted node-linker`. Ported as `installing_with_hoisted_node_linker` in `crates/cli/tests/hoisted_node_linker.rs` (real dirs at root + version-conflict nesting + `.modules.yaml` linker). The rimraf-then-reinstall re-add tail is the partial-install path (pnpm/pacquet#433) and is omitted.
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:45` `installing with hoisted node-linker and no lockfile`. Ported as `installing_with_hoisted_node_linker_and_no_lockfile` (real dir + no `pnpm-lock.yaml` when `lockfile: false`).
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:61` `overwriting (is-positive@3.0.0 with is-positive@latest)`. Ported as `overwriting_is_positive_with_latest` in `crates/cli/tests/hoisted_node_linker.rs` (`@pnpm.e2e/dep-of-pkg-with-1-dep` pinned-then-`@latest` stands in for upstream's fixture).
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:83` `overwriting existing files in node_modules`. Ported as `overwriting_existing_files_in_node_modules` (a squatting symlink is replaced by the real package directory).
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:97` `preserve subdeps on update`. Ported as `preserve_subdeps_on_update` (the nested conflict copy survives the parent's update via the hoisted linker's previous-graph diff).
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:119` `adding a new dependency to one of the workspace projects`. Ported as `adding_a_new_dependency_to_a_workspace_project`.
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:172` `installing the same package with alias and no alias`. Ported as `installing_same_package_with_alias_and_no_alias` (explicit range selectors stand in for upstream's dist-tag setup).
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:187` `run pre/postinstall scripts. bin files should be linked in a hoisted node_modules`. Ported as `run_pre_and_postinstall_scripts_and_link_bins` in `crates/cli/tests/hoisted_node_linker.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:210` `running install scripts in a workspace that has no root project`. Ported as `running_install_scripts_in_workspace_without_root_project`.
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:229` `hoistingLimits should prevent packages to be hoisted`. Ported as `hoisting_limits_prevents_hoisting` (`hoistingLimits: dependencies`). Pacquet's `hoistingLimits` config was migrated from the raw locator map to the `none`/`workspaces`/`dependencies` enum to match the pnpm CLI setting, and `real-hoist`'s border semantics were corrected (a name in the limits is a subtree border whose descendants stay nested, matching the `@yarnpkg/nm` hoister).
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:247` `externalDependencies should prevent package from being hoisted to the root`. Ported as `external_dependencies_prevents_hoisting_to_root`.
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:264` `linking bins of local projects when node-linker is set to hoisted`. Ported as `linking_bins_of_local_projects`.
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:314` `peerDependencies should be installed when autoInstallPeers is set to true and nodeLinker is set to hoisted`. Ported as `peer_dependencies_installed_with_auto_install_peers`.
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:329` `installing with hoisted node-linker a package that is a peer dependency of itself`. Ported as `package_that_is_peer_dependency_of_itself` (asserts the self-peer is absent from the lockfile's `peerDependencies`).
- [x] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:87` `install only the dependencies of the specified importer, when node-linker is hoisted` — ported as `install_only_dependencies_of_specified_importer_with_hoisted_linker` in `crates/cli/tests/hoisted_node_linker.rs` (matching upstream, only the positive assertions are pinned — upstream's "unselected dependency is absent" tail is a TODO there too).

Frozen/headless cross-coverage:

- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:594` `install optional dependency for the supported architecture set by the user (nodeLinker=%s)` includes hoisted frozen install — covered by `install_optional_dependency_for_the_supported_architectures` in `crates/cli/tests/optional_dependencies.rs`, which loops both linkers and replays via `--frozen-lockfile`.
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:24` `patch package with exact version` includes frozen hoisted reinstall — ported with the scenario (see Support `patchedDependencies`), which replays the frozen install under both linkers and asserts the layout each one produces (symlink vs. real directory) so the hoisted leg cannot pass as an isolated install.
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:120` `patch package with version range` includes frozen hoisted reinstall — same coverage.
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:297` `patch package when scripts are ignored` includes frozen hoisted reinstall — same coverage.
- [x] `TypeScript repo: installing/deps-installer/test/install/patch.ts:386` `patch package when the package is not in allowBuilds list` includes frozen hoisted reinstall — same coverage.

Pacquet also keeps `hoisted_patch_reaches_every_nested_copy_of_a_package` in `crates/cli/tests/patch.rs`: a package the walker nests under several consumers must be patched at every one of them, on both the fresh and the cache-warm frozen path. See the Rust port note under Support `patchedDependencies` for the bug it pins.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:579` `run pre/postinstall scripts in a workspace that uses node-linker=hoisted` — ported as `run_pre_and_postinstall_scripts_in_a_workspace_with_hoisted_linker` in `crates/cli/tests/hoisted_node_linker.rs`. The layout tail — no nested copy for consumers of the version that won the root slot — is stubbed in `known_failures::hoisted_workspace_layout_does_not_duplicate_root_version`: pacquet materializes every project's direct dep under the project as well.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:686` `run pre/postinstall scripts in a project that uses node-linker=hoisted. Should not fail on repeat install` — ported as `lifecycle_scripts_do_not_fail_on_repeat_hoisted_install` in `crates/cli/tests/hoisted_node_linker.rs`.
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:859` `installing with node-linker=hoisted`. Ported as `installing_with_hoisted_node_linker_frozen` in `crates/cli/tests/hoisted_node_linker.rs` — seeds the lockfile with a fresh install, tears down `node_modules`, then replays via `--frozen-lockfile` and asserts the real-dir + version-conflict-nesting layout.
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:873` `installing in a workspace with node-linker=hoisted`. Ported as `installing_in_a_workspace_with_hoisted_node_linker_frozen` — a frozen workspace replay where the root importer's `ms@2.1.3` wins the top-level slot and a project's conflicting `ms@2.0.0` nests under the project (the root-deps-rank-first preference landed in `real-hoist`).

Rust port notes:

- Treat hoisted linker as its own milestone. Its tests overlap with optional deps, patches, lifecycle scripts, bins, and workspaces.

## Support The Global Virtual Store Dir

Ported in `crates/cli/tests/global_virtual_store.rs`. Upstream drives the
primary suite through the programmatic `install()` API; pacquet's
equivalent surface is the CLI, so the ports assert the same on-disk
contract instead of the call counts upstream can reach by patching
`storeController.fetchPackage`.

Primary tests:

- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:21` `using a global virtual store` includes reinstall with `frozenLockfile: true` — ported as `using_a_global_virtual_store` (covers the CLI-level `:11` case too).
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:63` `reinstall from warm global virtual store after deleting node_modules` deletes `node_modules`, keeps GVS warm, and reinstalls frozen — ported as `reinstall_from_warm_global_virtual_store_after_deleting_node_modules`. Upstream's `fetchPackage` spy has no CLI equivalent; the port asserts the observable consequence instead (the warm slot is reused rather than materialized beside a second hash directory, and the whole project tree including `.bin` is restored from it).
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:107` `modules are correctly updated when using a global virtual store` — ported as `modules_are_correctly_updated_when_using_a_global_virtual_store`.
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:132` `GVS hashes are engine-agnostic for packages not in allowBuilds` — ported as `gvs_hashes_are_engine_agnostic_for_packages_not_in_allow_builds`.
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:172` `GVS hashes are stable when allowBuilds targets an unrelated package` — ported as `gvs_hashes_are_stable_when_allow_builds_targets_an_unrelated_package`.
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:205` `GVS re-links when allowBuilds changes` — ported as `gvs_relinks_when_allow_builds_changes`, including the `.modules.yaml` half (pacquet now persists `allowBuilds`, which it previously left unset).
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:250` `GVS successful build creates package directory with build artifacts` — ported as `gvs_successful_build_creates_package_directory_with_build_artifacts`, including removal of `.pnpm-needs-build` after a successful build and exclusion of the marker from side-effects uploads.
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:290` `GVS: approve-builds scenario — install with no builds, then reinstall with allowBuilds` — ported as `gvs_approve_builds_scenario_moves_artifacts_to_a_new_hash_dir`.
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:338` `GVS build failure cleans up broken package directory` — ported as `gvs_build_failure_cleans_up_broken_package_directory`; the cleanup itself is new in pacquet (`discard_failed_global_virtual_store_slot`).
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:367` `GVS rebuilds successfully after simulated build failure cleanup` — ported as `gvs_rebuilds_successfully_after_simulated_build_failure_cleanup`.
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:411` `GVS .pnpm-needs-build marker triggers re-import on next install` — ported as `needs_build_marker_triggers_reimport_on_next_install`; pacquet now imports the marker with a buildable GVS package, forces marked warm slots through re-import and rebuild, removes it after success, and excludes it from side-effects uploads.
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:461` `injected local packages work with global virtual store` — ported as `injected_local_packages_work_with_global_virtual_store` (the `.modules.yaml.injectedDeps` half; the materialization half is `injected_workspace_dep_with_dedupe_off_materialises_under_gvs` in `crates/cli/tests/dedupe_injected_deps.rs`).
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:539` `virtualStoreOnly populates standard virtual store without importer symlinks` — ported as `virtual_store_only_populates_standard_virtual_store_without_importer_symlinks`.
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:559` `virtualStoreOnly with enableModulesDir=false throws config error (standard virtual store)` — ported as `virtual_store_only_with_no_modules_dir_is_a_config_conflict`.
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:571` `virtualStoreOnly with enableModulesDir=false works when GVS is enabled` — ported as `virtual_store_only_with_no_modules_dir_works_when_gvs_is_enabled`.
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:605` `virtualStoreOnly with GVS populates global virtual store without importer links` — ported as `virtual_store_only_with_gvs_populates_the_store_without_importer_links`.
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:635` `virtualStoreOnly with frozenLockfile populates virtual store without importer symlinks` — ported as `virtual_store_only_with_frozen_lockfile_populates_the_gvs_without_importer_symlinks`.
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:677` `virtualStoreOnly with frozenLockfile populates standard virtual store without importer symlinks` — ported as `virtual_store_only_with_frozen_lockfile_populates_the_standard_store`.
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:708` `virtualStoreOnly suppresses hoisting even with explicit hoistPattern` — ported as `virtual_store_only_suppresses_hoisting_even_with_explicit_hoist_pattern`.

The seven `virtualStoreOnly` items required implementing the setting: it
had no `Config` field before (nor did `enableModulesDir`), so none of the
behavior was reachable end to end. Pacquet-only
`ordinary_install_after_virtual_store_only_completes_the_linking` pins the
follow-up contract upstream encodes as the `!modules.virtualStoreOnly`
guards in `validateModules.ts` and has no test for.

CLI-level tests:

- [x] `TypeScript repo: pnpm/test/install/globalVirtualStore.ts:11` `using a global virtual store` — same scenario as the primary `:21`; covered by `using_a_global_virtual_store`.
- [x] `TypeScript repo: pnpm/test/install/globalVirtualStore.ts:34` `approve-builds updates GVS symlinks and runs builds at correct hash directory` — ported as `approve_builds_updates_gvs_symlinks_and_runs_builds_at_the_new_hash_dir`, driving the real `approve-builds` command.
- [x] `TypeScript repo: pnpm/test/install/globalVirtualStore.ts:80` `warm GVS reinstall skips internal linking` — same scenario as the primary `:63`; covered by `reinstall_from_warm_global_virtual_store_after_deleting_node_modules`.

The two remaining upstream CLI cases (`switching from non-GVS to GVS
replaces stale hoisted symlinks`, `the post-install build step preserves
the global virtual store directory of a workspace package`) are not listed
above and remain unported.

## Link Dependency Binaries

Primary frozen/headless tests:

- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:331` `lifecycle scripts run before linking bins` verifies generated bins after frozen reinstall — ported as `lifecycle_scripts_run_before_linking_bins` in `crates/cli/tests/lifecycle_scripts.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:372` `bins are linked even if lifecycle scripts are ignored` — ported as `bins_linked_even_if_scripts_ignored` in `crates/cli/tests/lifecycle_scripts.rs` (including the frozen-reinstall tail).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:567` `the hoisted packages should not override the bin files of the direct dependencies` verifies public hoist bin precedence after frozen reinstall. Ported as `hoisted_packages_dont_override_direct_dep_bins` in `crates/cli/tests/hoist.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:1902` `link the bin file of a workspace project that is created by a lifecycle script` verifies workspace bin linking after frozen reinstall. Ported as `link_bin_of_workspace_project_created_by_lifecycle_script` in `crates/cli/tests/multiple_importers.rs`.

Supporting tests:

- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:54` `installing a simple project` verifies a dependency bin in headless install — ported as `frozen_reinstall_writes_modules_manifest_current_lockfile_and_bins` in `crates/cli/tests/install_state.rs`.
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:569` `installing with hoistPattern=*` verifies private hoisted `.bin/hello-world-js-bin` — covered by `private_hoist_links_bins` in `crates/cli/tests/hoist.rs` (asserts the private `.bin` under `.pnpm/node_modules` after a frozen install).
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:628` `installing with publicHoistPattern=*` verifies public `.bin/hello-world-js-bin` — covered by `public_hoist_bin_is_linked_via_root_bin_dir` in `crates/cli/tests/hoist.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/misc.ts:1130` `installing with no symlinks with PnP` verifies `.bin` exists with no symlink layout — ported as `pnp_install_without_symlinks_still_writes_modules_manifest_and_bin_directory` in `crates/cli/tests/install_state.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:187` `run pre/postinstall scripts. bin files should be linked in a hoisted node_modules`. Ported as `run_pre_and_postinstall_scripts_and_link_bins` in `crates/cli/tests/hoisted_node_linker.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:264` `linking bins of local projects when node-linker is set to hoisted`. Ported as `linking_bins_of_local_projects`.

Rust port notes:

- Port direct dependency bins first.
- Then add hoisted-bin precedence and lifecycle-created bins.

## Existing `node_modules` With Existing Packages

Primary frozen/headless tests:

- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:237` `installing non-prod deps then all deps` verifies headless repeat install adds missing dependency groups and updates install state — ported as `installing_non_prod_deps_then_all_deps` in `crates/cli/tests/repeat_install.rs` (this port also fixed `Lockfile::is_empty` misreading dev-only installs and deleting their current lockfile).
- [x] `TypeScript repo: installing/deps-installer/test/install/misc.ts:844` `reinstalls missing packages to node_modules during headless install` starts with existing install, removes package links/store locations, and verifies install repairs `node_modules` — ported as `reinstalls_missing_packages_during_headless_install` in `crates/cli/tests/repeat_install.rs` (asserts the `pnpm:_broken_node_modules` emission; the frozen no-op short-circuit now probes the tree so the repair path is reachable).
- [x] `TypeScript repo: installing/deps-installer/test/lockfile.ts:547` `repeat install with no inner lockfile should not rewrite packages in node_modules` verifies reinstall keeps existing packages usable when `node_modules/.pnpm/lock.yaml` is absent — ported as `repeat_install_with_no_inner_lockfile_keeps_packages_usable` in `crates/cli/tests/repeat_install.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:24` `should hoist dependencies` verifies repeat installs preserve existing hoisted packages under frozen/headless install — covered by `should_hoist_dependencies_repeat_install_preserves_map` in `crates/cli/tests/hoist.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/packageImportMethods.ts:31` `packages are updated in node_modules, when packageImportMethod is set to copy and modules manifest and current lockfile are incorrect` corrupts both install-state files and verifies `node_modules` is repaired — ported as `stale_state_files_do_not_stop_node_modules_from_being_repaired` in `crates/cli/tests/current_lockfile.rs`.

Supporting tests:

- [x] `TypeScript repo: installing/deps-installer/test/install/misc.ts:784` `rewrites node_modules created by npm` is relevant to pre-existing `node_modules`, but not frozen/headless — ported as `rewrites_node_modules_created_by_npm` in `crates/cli/tests/install_state.rs`.
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:432` `available packages are used when node_modules is not clean` is headless-restorer behavior around dirty `node_modules` — ported as `available_packages_used_when_node_modules_not_clean` in `crates/cli/tests/repeat_install.rs` (a wiped store pins that on-disk packages are reused, not refetched).
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:469` `available packages are relinked during forced install` covers force-path relinking with existing packages — ported as `available_packages_are_relinked_during_forced_install` in `crates/cli/tests/repeat_install.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:63` `reinstall from warm global virtual store after deleting node_modules` repairs project links from a warm GVS — covered by `reinstall_from_warm_global_virtual_store_after_deleting_node_modules`.
- [x] `TypeScript repo: pnpm/test/install/globalVirtualStore.ts:80` `warm GVS reinstall skips internal linking` is CLI-level existing-`node_modules`/warm-GVS coverage — covered by `reinstall_from_warm_global_virtual_store_after_deleting_node_modules`.

Rust port notes:

- First test target: do not assume clean `node_modules`.
- Preserve user/unrelated files and repair missing package links without rewriting everything unnecessarily.

## Write And Update Current Lockfile At `node_modules/.pnpm/lock.yaml`

Primary tests:

- [x] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:21` `using a global virtual store` verifies `node_modules/.pnpm/lock.yaml` exists after install and frozen reinstall — the current-lockfile half is ported as `a_global_virtual_store_install_still_writes_the_current_lockfile` in `crates/cli/tests/current_lockfile.rs`; the GVS layout assertions belong to the GVS section.
- [x] `TypeScript repo: installing/deps-installer/test/install/packageExtensions.ts:16` `manifests are extended with fields specified by packageExtensions` — split into pacquet's `install::tests::fresh_install_applies_package_extensions_to_dependency_manifest` (verifies the extension lands in the lockfile's `packages` block AND `packageExtensionsChecksum` is written) and `install::tests::frozen_lockfile_errors_when_package_extensions_drift_from_lockfile` (frozen-install drift gate). Current-lockfile round-trip parity is covered by `current_lockfile`'s clone of `package_extensions_checksum`.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:547` `dependency should not be added to current lockfile if it was not built successfully during headless install` verifies failed build does not update current lockfile — ported as `a_failed_build_writes_no_current_lockfile` in `crates/cli/tests/current_lockfile.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:74` `skip optional dependency that does not support the current OS` verifies current lockfile package set matches wanted lockfile while skipped packages are tracked — covered by `skip_optional_dependency_that_does_not_support_the_current_os` in `crates/cli/tests/optional_dependencies.rs`, which asserts both the current-lockfile package set and the skip set.
- [x] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:208` `install only the dependencies of the specified importer. The current lockfile has importers that do not exist anymore` — ported as `stale_current_lockfile_importers_are_retained_on_subset_install` in `crates/cli/tests/multiple_importers.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:730` `current lockfile contains only installed dependencies when adding a new importer to workspace with shared lockfile` — ported as `current_lockfile_contains_only_installed_dependencies` in `crates/cli/tests/multiple_importers.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/packageImportMethods.ts:31` `packages are updated in node_modules, when packageImportMethod is set to copy and modules manifest and current lockfile are incorrect` covers incorrect current lockfile repair — ported as `stale_state_files_do_not_stop_node_modules_from_being_repaired` in `crates/cli/tests/current_lockfile.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/lockfile.ts:368` `subdeps are updated on repeat install if outer pnpm-lock.yaml does not match the inner one` tests wanted/current lockfile divergence — ported as `subdeps_updated_when_outer_lockfile_diverges_from_inner` in `crates/cli/tests/repeat_install.rs` (a direct-pin bump regenerating only the outer lockfile produces the divergence).
- [x] `TypeScript repo: installing/deps-installer/test/lockfile.ts:547` `repeat install with no inner lockfile should not rewrite packages in node_modules` covers missing current lockfile on repeat install — see `repeat_install_with_no_inner_lockfile_keeps_packages_usable` in `crates/cli/tests/repeat_install.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/lockfile.ts:1007` `use current pnpm-lock.yaml as initial wanted one, when wanted was removed` covers recovering from current lockfile when wanted lockfile is gone — ported as `a_deleted_wanted_lockfile_is_regenerated_from_the_current_one` in `crates/cli/tests/current_lockfile.rs`, which asserts the regenerated `pnpm-lock.yaml` is byte-identical (a re-resolve would be free to move the ranges). `install_regenerates_lockfile_from_node_modules_when_wanted_is_missing` in `crates/cli/tests/install.rs` covers the same recovery via the `node_modules` snapshot.
- [x] `TypeScript repo: installing/deps-installer/test/lockfile.ts:1351` `a broken private lockfile is ignored` covers malformed `node_modules/.pnpm/lock.yaml` — ported as `a_broken_current_lockfile_is_ignored_with_a_warning` in `crates/cli/tests/current_lockfile.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/lockfile.ts:1324` `a lockfile with duplicate keys causes an exception, when frozenLockfile is true` covers frozen lockfile parse/validation failure — ported as `a_wanted_lockfile_with_duplicate_keys_fails_a_frozen_install` in `crates/cli/tests/current_lockfile.rs`.

Supporting tests:

- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:54` `installing a simple project` verifies current lockfile exists after headless install — ported as `a_frozen_install_writes_the_current_lockfile` in `crates/cli/tests/current_lockfile.rs`.
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:165` `installing with package manifest ignored` verifies filtered current lockfile package contents — ported as `the_current_lockfile_is_filtered_to_the_installed_groups` in `crates/cli/tests/current_lockfile.rs` via the CLI-expressible group filters (upstream's `ignorePackageManifest` exists only in the programmatic API).
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:189` `installing only prod package with package manifest ignored` verifies filtered current lockfile package contents — the `--prod` phase of `the_current_lockfile_is_filtered_to_the_installed_groups`.
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:213` `installing only dev package with package manifest ignored` verifies filtered current lockfile package contents. `headless_install_include_filtering_excludes_production_group` in `crates/cli/tests/optional_dependencies.rs` covers the dev-only install's on-disk result; the current-lockfile assertion is unreachable because a dev-only importer reads as empty to `Lockfile::is_empty` (upstream's `isEmptyLockfile` has the same shape), so the file is deleted rather than written.
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:789` `installing in a workspace` verifies current lockfile is filtered after subset workspace headless install — covered by `subset_headless_install_keeps_other_projects_packages_in_current_lockfile` in `crates/cli/tests/multiple_importers.rs`.

Rust port notes:

- Port the simple write first, then filtered lockfiles, then negative failed-build behavior.
- Ported tests live in `crates/cli/tests/current_lockfile.rs`; the workspace-subset cases live with the rest of the multi-importer coverage in `crates/cli/tests/multiple_importers.rs`.

## Progress Reporting Matching pnpm

Reporter unit tests:

- [x] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:25` `prints progress beginning` — ported as `prints_progress_beginning` in `crates/default-reporter/tests/render.rs`.
- [x] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:50` `prints progress without added packages stats` — ported as `prints_progress_without_added_packages_stats` in `crates/default-reporter/tests/render.rs`.
- [x] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:78` `prints all progress stats` — ported as `prints_all_progress_stats` in `crates/default-reporter/tests/render.rs`.
- [x] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:119` `prints progress beginning of node_modules from not cwd` — ported as `prints_progress_beginning_for_node_modules_outside_cwd` in `crates/default-reporter/tests/render.rs`.
- [x] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:144` `prints progress beginning of node_modules from not cwd, when progress prefix is hidden` — ported as `hides_progress_prefix_for_node_modules_outside_cwd` in `crates/default-reporter/tests/render.rs`.
- [x] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:172` `prints progress beginning when appendOnly is true` — ported as `prints_progress_beginning_in_append_only_mode` in `crates/default-reporter/tests/render.rs`.
- [x] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:200` `prints progress beginning during recursive install` — ported as `prints_progress_beginning_during_recursive_install` in `crates/default-reporter/tests/render.rs`.
- [x] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:228` `prints progress on first download` — ported as `prints_progress_on_first_download` in `crates/default-reporter/tests/render.rs`.
- [x] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:262` `moves fixed line to the end` — ported as `moves_fixed_progress_line_to_the_end` in `crates/default-reporter/tests/render.rs`.
- [x] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:307` `prints "Already up to date"` — ported as `already_up_to_date_pnpm_log_renders` in `crates/default-reporter/tests/render.rs`.
- [x] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:324` `prints progress of big files download` — ported as `prints_progress_of_big_files_download` in `crates/default-reporter/tests/render.rs`.

Install reporter coverage:

- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:54` `installing a simple project` asserts headless reporter events: stats, stage, package-manifest, and resolved logs — ported in `crates/package-manager/src/install/tests.rs::should_install_dependencies`.

Rust port notes:

- Port event-shape tests separately from terminal rendering.
- For terminal rendering, snapshot exact text only after event parity exists.
- Landed scaffolding: `progress_line_counts_each_status` / `stats_render_packages_line_and_bar` (`crates/default-reporter/tests/render.rs`) and `progress_event_matches_pnpm_wire_shape` / `fetching_progress_event_matches_pnpm_wire_shape` (`crates/reporter/src/tests.rs`) cover the wire shape and basic rendering; the unchecked items above are the remaining upstream-specific scenarios.

## Support Proper Auth

Install tests:

- [x] `TypeScript repo: installing/deps-installer/test/install/auth.ts:14` `a package that need authentication` — `bearer_auth_is_used_for_metadata_tarballs_and_cold_frozen_reinstall` in `crates/cli/tests/auth.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/auth.ts:52` `installing a package that need authentication, using password` — `username_and_password_authenticates_install` in `crates/cli/tests/auth.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/auth.ts:73` `a package that need authentication, legacy way` — `legacy_basic_auth_authenticates_install` in `crates/cli/tests/auth.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/auth.ts:94` `a scoped package that need authentication specific to scope` — `package_scope_bearer_auth_wins_for_scoped_install` in `crates/cli/tests/auth.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/auth.ts:142` `a scoped package that need legacy authentication specific to scope` — `package_scope_legacy_auth_wins_for_scoped_install` in `crates/cli/tests/auth.rs`.
- [x] `TypeScript repo: installing/deps-installer/test/install/auth.ts:190` `a package that need authentication reuses authorization tokens for tarball fetching` — the authenticated tarball assertion in `bearer_auth_is_used_for_metadata_tarballs_and_cold_frozen_reinstall`.
- [x] `TypeScript repo: installing/deps-installer/test/install/auth.ts:216` `a package that need authentication reuses authorization tokens for tarball fetching when meta info is cached` — the cold-store frozen reinstall in `bearer_auth_is_used_for_metadata_tarballs_and_cold_frozen_reinstall`.

Auth header tests:

- [x] `TypeScript repo: network/auth-header/test/getAuthHeadersFromConfig.test.ts:32` `should convert auth token to Bearer header`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeadersFromConfig.test.ts:42` `should convert basicAuth to Basic header`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeadersFromConfig.test.ts:50` `should handle default registry auth (empty key)`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeadersFromConfig.test.ts:58` `should execute tokenHelper`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeadersFromConfig.test.ts:66` `should prepend Bearer to raw token from tokenHelper`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeadersFromConfig.test.ts:74` `should throw an error if the token helper fails`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeadersFromConfig.test.ts:79` `should throw an error if the token helper returns an empty token`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeaderByURI.ts:11` `getAuthHeaderByURI()`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeaderByURI.ts:22` `getAuthHeaderByURI() basic auth without settings`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeaderByURI.ts:30` `getAuthHeaderByURI() basic auth with settings`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeaderByURI.ts:41` `getAuthHeaderByURI() https port 443 checks`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeaderByURI.ts:49` `getAuthHeaderByURI() when default ports are specified`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeaderByURI.ts:61` `getAuthHeaderByURI() when the registry has pathnames`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeaderByURI.ts:72` `getAuthHeaderByURI() with default registry auth`

Auth config parsing and precedence tests:

- [x] `TypeScript repo: config/reader/test/index.ts:481` `auth tokens from pnpm auth file override ~/.npmrc` — ported as `npmrc_auth_file_outranks_userconfig` (plus `npmrc_auth_file_override_supplies_auth` and `user_auth_token_pins_to_its_own_file_registry`) in `crates/config/src/tests.rs`.
- [x] `TypeScript repo: config/reader/test/index.ts:523` `workspace .npmrc overrides pnpm auth file` — `workspace_npmrc_overrides_global_auth_file` in `crates/config/src/tests.rs`.
- [x] `TypeScript repo: config/reader/test/parseCreds.test.ts:15` `authToken`
- [x] `TypeScript repo: config/reader/test/parseCreds.test.ts:23` `authPairBase64`
- [x] `TypeScript repo: config/reader/test/parseCreds.test.ts:49` `authUsername and authPassword`
- [x] `TypeScript repo: config/reader/test/parseCreds.test.ts:69` `tokenHelper`

Fetcher tests:

- [x] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:349` `throw error when accessing private package w/o authorization` — `tarball_authorization_failure_is_reported` in `crates/cli/tests/auth.rs`.
- [x] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:409` `accessing private packages` — the authenticated tarball assertions in `crates/cli/tests/auth.rs`.
- [x] `TypeScript repo: network/fetch/test/fetchFromRegistry.test.ts:62` `authorization headers are removed before redirection if the target is on a different host` — `authorization_is_removed_on_cross_origin_redirect` in `crates/network/src/tests.rs`.
- [x] `TypeScript repo: network/fetch/test/fetchFromRegistry.test.ts:90` `authorization headers are not removed before redirection if the target is on the same host` — `authorization_is_retained_on_same_origin_redirect` in `crates/network/src/tests.rs`.
- [x] `TypeScript repo: resolving/npm-resolver/test/index.ts:934` `error is thrown when package needs authorization` — `metadata_authorization_failure_is_reported` in `crates/cli/tests/auth.rs`.

Rust port notes:

- Frozen install still fetches tarballs when the store is cold, so auth applies even without resolution; `bearer_auth_is_used_for_metadata_tarballs_and_cold_frozen_reinstall` exercises that path.
- Header matching, token helpers, install authentication, and fetch failures are covered at their corresponding layers.

## Support pnpm Proxy Settings

Proxy dispatcher tests:

- [x] `TypeScript repo: network/fetch/test/dispatcher.test.ts:62` `returns ProxyAgent for httpProxy with http target` — behavioral equivalent `mockito_integration_http_proxy_forwards_request_with_basic_auth` in `crates/network/src/tests.rs` (proves the http proxy actually carries the request; undici's Agent/ProxyAgent split has no Rust analog).
- [x] `TypeScript repo: network/fetch/test/dispatcher.test.ts:69` `returns ProxyAgent for httpsProxy with https target` — `https_target_uses_configured_proxy` in `crates/network/src/tests.rs` proves the HTTPS target reaches the configured proxy with CONNECT.
- [x] `TypeScript repo: network/fetch/test/dispatcher.test.ts:76` `adds protocol prefix when proxy URL has none` — `parse_proxy_url_auto_prefixes_missing_scheme` in `crates/network/src/tests.rs`.
- [x] `TypeScript repo: network/fetch/test/dispatcher.test.ts:84` `throws PnpmError for invalid proxy URL` — `parse_proxy_url_invalid_returns_invalid_proxy_error` plus `for_installs_with_invalid_proxy_url_errors` in `crates/network/src/tests.rs`.
- [x] `TypeScript repo: network/fetch/test/dispatcher.test.ts:92` `proxy with authentication credentials` — `strip_userinfo_decodes_user_and_password` plus `mockito_integration_http_proxy_forwards_request_with_basic_auth` in `crates/network/src/tests.rs`.
- [x] `TypeScript repo: network/fetch/test/dispatcher.test.ts:101` `returns Agent (not ProxyAgent) for socks5 proxy` — behavioral equivalents `parse_proxy_url_socks_schemes_pass_through` and `for_installs_with_socks_proxy_url_builds` in `crates/network/src/tests.rs` (the Agent/ProxyAgent distinction has no Rust analog).
- [x] `TypeScript repo: network/fetch/test/dispatcher.test.ts:111` `returns Agent for socks4 proxy` — same coverage as the socks5 entry.
- [x] `TypeScript repo: network/fetch/test/dispatcher.test.ts:119` `returns Agent for socks proxy with https target` — same coverage as the socks5 entry.
- [x] `TypeScript repo: network/fetch/test/dispatcher.test.ts:127` `SOCKS proxy dispatchers are cached` — pacquet builds the proxy dispatcher once inside the `reqwest::Client` retained by `ThrottledClient`, so there is no separate dispatcher-cache identity to test.
- [x] `TypeScript repo: network/fetch/test/dispatcher.test.ts:134` `SOCKS proxy can connect through a real SOCKS5 server` — `socks5_proxy_connects_to_real_target` in `crates/network/src/tests.rs`.
- [x] `TypeScript repo: network/fetch/test/dispatcher.test.ts:192` `bypasses proxy when noProxy matches hostname` — `no_proxy_matcher_reverse_dot_match` in `crates/network/src/tests.rs` (exact-host probe).
- [x] `TypeScript repo: network/fetch/test/dispatcher.test.ts:202` `bypasses proxy when noProxy matches domain suffix` — `no_proxy_matcher_reverse_dot_match` (subdomain probes).
- [x] `TypeScript repo: network/fetch/test/dispatcher.test.ts:211` `does not bypass proxy when noProxy does not match` — negative probes in `no_proxy_matcher_reverse_dot_match` and `no_proxy_matcher_multiple_entries`.
- [x] `TypeScript repo: network/fetch/test/dispatcher.test.ts:219` `bypasses proxy when noProxy is true` — `no_proxy_bypass_short_circuits_every_host` in `crates/network/src/tests.rs`.
- [x] `TypeScript repo: network/fetch/test/dispatcher.test.ts:228` `handles comma-separated noProxy list` — `no_proxy_matcher_multiple_entries` in `crates/network/src/tests.rs` plus `cascade_no_proxy_comma_list_trimmed` in `crates/config/src/npmrc_auth/tests.rs`.

Config tests:

- [x] `TypeScript repo: config/reader/test/index.ts:978` `getConfig() converts noproxy to noProxy` — covered by `no_proxy_and_noproxy_aliases_last_wins` in `crates/config/src/npmrc_auth/tests.rs`.
- [x] `TypeScript repo: config/reader/test/index.ts:1514` `reads proxy settings from global config.yaml` — `global_config_yaml_supplies_proxy_settings` in `crates/config/src/tests.rs`.
- [x] `TypeScript repo: config/reader/test/index.ts:1540` `proxy settings from global config.yaml override .npmrc` — `global_config_yaml_proxy_overrides_project_npmrc` in `crates/config/src/tests.rs`.
- [x] `TypeScript repo: config/reader/test/index.ts:1567` `CLI flags override proxy settings from global config.yaml` — `pnpm_config_proxy_overrides_global_config_yaml` in `crates/config/src/tests.rs`, plus plain and dotted flag coverage in `crates/cli/src`.
- [x] `TypeScript repo: config/reader/test/index.ts:1592` `proxy settings are still read from .npmrc` — `project_npmrc_proxy_settings_are_preserved` in `crates/config/src/tests.rs`.
- [x] `TypeScript repo: config/commands/test/configSet.test.ts:875` `config set --global https-proxy writes to config.yaml, not auth.ini` — ported as `set_global_https_proxy_writes_config_yaml_not_auth_ini` in `crates/cli/src/cli_args/config/tests.rs`.
- [x] `TypeScript repo: config/commands/test/configSet.test.ts:902` `config set --global httpProxy writes to config.yaml` — `set_global_http_proxy_writes_config_yaml` in `crates/cli/src/cli_args/config/tests.rs`.
- [x] `TypeScript repo: config/commands/test/configSet.test.ts:928` `config set --global no-proxy writes to config.yaml` — `set_global_no_proxy_writes_config_yaml` in `crates/cli/src/cli_args/config/tests.rs`.

Rust port notes:

- No direct frozen/headless install proxy test was found. Use these as the parity map for config and network plumbing.
- Add an install-level cold-store frozen test after the Rust fetcher supports proxy injection.

## Installation Of Runtimes

The runtime lockfile *format* (importer `version: runtime:<ver>`, the
`packages[node@runtime:<ver>].version: <ver>` field, and the
`variants[].resolution.bin: { node: … }` map asserted in
`nodeRuntime.ts:236-269`) is covered at pacquet's adapter/resolver layer by
`dependencies_graph_to_lockfile::tests::runtime_dependency_strips_importer_prefix_and_records_package_version`
and `node_resolver::tests::bin_spec_is_a_named_map`. The command-level ports
use local HTTP runtime archives so they exercise fetching, integrity checks,
runtime manifest synthesis, bin linking, and frozen/offline reinstall without
depending on external release services.

Node runtime tests:

- [x] `TypeScript repo: installing/deps-installer/test/install/nodeRuntime.ts:209` `installing Node.js runtime` includes frozen/offline reinstall after deleting `node_modules`.
- [x] `TypeScript repo: installing/deps-installer/test/install/nodeRuntime.ts:332` `installing node.js runtime fails if offline mode is used and node.js not found locally`.
- [x] `TypeScript repo: installing/deps-installer/test/install/nodeRuntime.ts:339` `installing Node.js runtime from RC channel`
- [x] `TypeScript repo: installing/deps-installer/test/install/nodeRuntime.ts:346` `installing Node.js runtime fails if integrity check fails` verifies frozen integrity failure.
- [x] `TypeScript repo: installing/deps-installer/test/install/nodeRuntime.ts:400` `installing Node.js runtime for the given supported architecture` includes frozen reinstall for target architecture.
- [x] `TypeScript repo: installing/deps-installer/test/install/nodeRuntime.ts:422` `installing Node.js runtime, when it is set via the engines field of a dependency`

Deno runtime tests:

- [x] `TypeScript repo: installing/deps-installer/test/install/denoRuntime.ts:111` `installing Deno runtime` includes frozen/offline reinstall.
- [x] `TypeScript repo: installing/deps-installer/test/install/denoRuntime.ts:217` `installing Deno runtime fails if offline mode is used and Deno not found locally`
- [x] `TypeScript repo: installing/deps-installer/test/install/denoRuntime.ts:224` `installing Deno runtime fails if integrity check fails`

Bun runtime tests:

- [x] `TypeScript repo: installing/deps-installer/test/install/bunRuntime.ts:128` `installing Bun runtime` includes frozen/offline reinstall.
- [x] `TypeScript repo: installing/deps-installer/test/install/bunRuntime.ts:215` `installing Bun runtime fails if offline mode is used and Bun not found locally`
- [x] `TypeScript repo: installing/deps-installer/test/install/bunRuntime.ts:222` `installing Bun runtime fails if integrity check fails`

Command-level tests:

- [x] `TypeScript repo: installing/commands/test/install.ts:124` `install Node.js when devEngines runtime is set with onFail=download`
- [x] `TypeScript repo: installing/commands/test/install.ts:160` `do not install Node.js when devEngines runtime is not set to onFail=download`
- [x] `TypeScript repo: pnpm/test/install/runtimeOnFail.ts:8` `runtimeOnFail=download causes Node.js to be downloaded even when the manifest does not set onFail`
- [x] `TypeScript repo: pnpm/test/install/runtimeOnFail.ts:31` `runtimeOnFail=ignore prevents Node.js download even when manifest sets onFail=download`

Runtime manifest/config conversion tests:

- [x] `TypeScript repo: config/reader/test/index.ts:85` `nodeVersion from config takes priority over devEngines.runtime`
- [x] `TypeScript repo: config/reader/test/index.ts:109` `runtimeOnFail=download overrides devEngines.runtime.onFail and adds node to devDependencies`
- [x] `TypeScript repo: config/reader/test/index.ts:138` `runtimeOnFail=ignore overrides an existing onFail=download and removes node from devDependencies`
- [x] `TypeScript repo: workspace/project-manifest-reader/test/index.ts:37` `readProjectManifest() converts devEngines runtime to devDependencies` — ported as `from_path_applies_convert_engines_runtime` (read path) and `convert_engines_runtime_lifts_devengines_runtime_into_devdependencies` (helper) in `crates/package-manifest/src/tests.rs`.
- [x] `TypeScript repo: workspace/project-manifest-reader/test/index.ts:68` `readProjectManifest() converts engines runtime to dependencies` — covered at helper level by `convert_engines_runtime_targets_dependencies_for_engines_field` in `crates/package-manifest/src/tests.rs` (the `from_path` read-path test asserts only the `devEngines` direction).

Rust port notes:

- Ported in `crates/cli/tests/install_runtimes.rs`, with conversion and resolver
  details pinned by the focused unit tests named above.

## Installation Of Git-Hosted Packages

Install tests:

The install-level ports build a local repo per test and reference it over `git+file://` (`GitRepoFixture` in `pacquet-testing-utils`), so the whole git install path runs without network access — the same technique upstream's `createGitPreparePackage` uses. That trades away the *host* archive identity (`gitHosted: true` tarball resolution), which is pinned separately at the resolver level; a `file:` repo resolves to `type: git`. They live in `crates/cli/tests/git_hosted_install.rs`.

- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:31` `from a github repo` — alias-less hosted shorthands are recognized before registry package-name parsing; `add_from_a_git_url_without_an_alias` covers the shared end-to-end add path without a public-service dependency.
- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:48` `from a github repo through URL` — ported as `add_from_a_git_url_without_an_alias` using a local git URL.
- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:61` `from a github repo with different name via named installation` — ported as `install_from_a_git_repo_with_a_different_name_via_named_installation`. Asserts the alias/`realName`/`dependencyType`/`version` on the `pnpm:root` event and both linked bins.
- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:105` `from a github repo with different name` — ported as `install_from_a_git_repo_with_a_different_name`.
- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:150` `a subdependency is from a github repo with different name` — ported as `registry_dependency_can_alias_a_git_dependency_that_provides_a_peer`; the pnpr fixture builder substitutes the committed hosted URL with a per-run local git URL in both the packument and tarball manifest.
- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:174` `from a git repo` — ported as `install_from_a_git_repo` (upstream reaches github over `git+ssh://` and self-skips on CI; the `file:` repo exercises the same non-host `type: git` branch). Also pins the bare-`git+…#<commit>` importer ref against pnpm 11.
- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:206` `from a github repo that has no package.json file` — covered at fetcher level by `crates/git-fetcher/src/fetcher/tests.rs::fetcher_handles_repo_without_package_json`. Full install-level test deferred until a non-resolver lockfile fixture lands.
- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:276` `re-adding a git repo with a different tag` — ported as `re_adding_a_git_repo_with_a_different_tag`.
- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:323` `should not update when adding unrelated dependency` — ported as `adding_an_unrelated_dependency_reuses_the_locked_git_commit`; unchanged git selectors reuse their exact lockfile package key instead of resolving a moving ref again.
- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:354` `git-hosted repository is not added to the store if it fails to be built` — ported as `git_hosted_repository_is_not_added_to_the_store_if_it_fails_to_be_built`.
- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:366` `from subdirectories of a git repo` — ported as the install-level `install_from_subdirectories_of_a_git_repo` (two `#path:` subdirectories of one repo); the fetcher-level `fetcher_packs_subfolder_when_path_set` remains.
- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:389` `no hash character for github subdirectory install` — ported as `no_hash_character_for_subdirectory_install` (`#path:/&<ref>` splits the ref out of the `path:` fragment).
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:311` `run prepare script for git-hosted dependencies` — ported as `run_prepare_script_for_git_hosted_dependencies` (asserts the full `preinstall,install,postinstall,prepare,preinstall,install,postinstall` order).

Fetcher/resolver/store tests:

- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:50` `fetch` — `crates/git-fetcher/src/fetcher/tests.rs::fetcher_imports_package_into_cas`.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:69` `fetch a package from Git sub folder` — `fetcher_packs_subfolder_when_path_set`.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:87` `prevent directory traversal attack when using Git sub folder` — `prepare_package::tests::safe_join_path_rejects_escapes` + `cas_io::tests::materialize_into_rejects_traversal`.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:108` `prevent directory traversal attack when using Git sub folder #2` — same coverage as above (`join_checked` rejects every non-`Normal` component variant).
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:129` `fetch a package from Git that has a prepare script` — `fetcher::tests::fetcher_runs_prepare_script_when_allowed`. The test calls `node`/`npm` directly; under-provisioned hosts fail loudly via the existing `.unwrap()` calls (see the "No 'tolerant' tests for missing tools" rule in `pnpm/AGENTS.md`).
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:150` `fetch a package without a package.json` — `fetcher_handles_repo_without_package_json`.
- [ ] `TypeScript repo: fetching/git-fetcher/test/index.ts:169` `fetch a big repository` — perf benchmark, not a correctness test; skip from the porting plan.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:183` `still able to shallow fetch for allowed hosts` — `fetcher::tests::fetcher_uses_shallow_fetch_for_allowed_hosts` (Unix only). A `/bin/sh` shim at `<tempdir>/shim/git` logs every invocation and fakes `rev-parse HEAD`; `PATH` is prepended for the test body via `unsafe { std::env::set_var }` (safe under `cargo nextest`'s one-process-per-test isolation) and the log is parsed to assert the `init` / `remote add origin <url>` / `fetch --depth 1 origin <commit>` sequence. The mirror `fetcher_clones_when_host_not_in_shallow_list` pins the non-shallow branch so the gate's polarity can't drift.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:212` `fail when preparing a git-hosted package` — `fetcher::tests::fetcher_surfaces_prepare_failure`. `node -e "process.exit(1)"` as the prepare script; expects `GitFetcherError::Prepare(PreparePackageError::LifecycleFailed)` carrying `ERR_PNPM_PREPARE_PACKAGE`.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:230` `fail when preparing a git-hosted package with a partial commit` — `fetcher_rejects_partial_commit_before_running_git` verifies the abbreviated commit is rejected even when the configured git binary does not exist.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:247` `do not build the package when scripts are ignored` — `fetcher_skips_build_when_ignore_scripts`.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:263` `block git package with prepare script` — `fetcher_blocks_build_when_not_allowed`.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:280` `allow git package with prepare script` — `fetcher::tests::fetcher_runs_prepare_when_allow_build_returns_true`. Mirror of the existing block-test (`index.ts:263`) with a per-(name, version) `allow_build` closure returning true; asserts the prepare script's marker file lands in `cas_paths`.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:304` `fetch only the included files` — `tarball_fetcher::tests::filters_files_outside_files_field` (same packlist code path).
- [ ] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:455` `fetch a big repository` — perf benchmark, not a correctness test; skip.
- [x] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:472` `fail when preparing a git-hosted package` — `tarball_fetcher::tests::surfaces_prepare_script_failure` expects the failing `prepare` lifecycle error.
- [x] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:490` `take only the files included in the package, when fetching a git-hosted package` — `tarball_fetcher::tests::filters_files_outside_files_field`.
- [x] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:534` `do not build the package when scripts are ignored` — git-fetcher equivalent covered (`fetcher_skips_build_when_ignore_scripts`); the tarball-side mirror is `fast_path_ignore_scripts_returns_input_without_queueing_row` in `crates/git-fetcher/src/tarball_fetcher/tests.rs`.
- [x] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:580` `use the subfolder when path is present` — `tarball_fetcher::tests::path_field_packs_only_subdirectory`.
- [x] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:610` `prevent directory traversal attack when path is present` — `tarball_path_traversal_attack_is_rejected`.
- [x] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:637` `fail when path is not exists` — `tarball_path_to_missing_subdir_is_rejected`.
- [x] `TypeScript repo: resolving/git-resolver/test/index.ts:188` `resolveFromGit() with sub folder` — ported as `path_suffix_appended_to_id_and_resolution` in `crates/resolving-git-resolver/src/git_resolver/tests.rs`.
- [x] `TypeScript repo: resolving/git-resolver/test/index.ts:211` `resolveFromGit() with both sub folder and branch` — ported as `sub_folder_and_branch_resolve_to_a_tarball_carrying_the_path` in `crates/resolving-git-resolver/src/git_resolver/tests.rs`.
- [x] `TypeScript repo: resolving/git-resolver/test/index.ts:482` `resolve a private repository using the HTTPS protocol without auth token` — ported as `private_https_repo_without_auth_falls_back_to_the_ssh_url` (the `FakeProbe::private_reachable_over` seam makes exactly one transport reachable).
- [x] `TypeScript repo: resolving/git-resolver/test/index.ts:526` `resolve a private repository using the HTTPS protocol and an auth token` — ported as `private_https_repo_with_an_auth_token_keeps_the_authenticated_url`. Fixing this found a divergence: pacquet resolved an auth-bearing private repo to the host's public `codeload` archive URL (`gitHosted: true` tarball) — a URL that carries none of the URL's credentials. Now `hosted: None` in the private-repo branch of `from_hosted_git` keeps it a `type: git` resolution against the authenticated remote, matching upstream's `tarball: undefined`.
- [x] `TypeScript repo: installing/package-requester/test/index.ts:884` `fetch a git package without a package.json` — covered alongside `fetching/git-fetcher/test/index.ts:150` via `fetcher_handles_repo_without_package_json`.
- [x] `TypeScript repo: installing/deps-installer/test/install/peerDependencies.ts:30` `don't fail when peer dependency is fetched from GitHub` — covered by `registry_dependency_can_alias_a_git_dependency_that_provides_a_peer`.
- [x] `TypeScript repo: installing/deps-installer/test/lockfile.ts:600` `updating package that has a github-hosted dependency` — ported as `updating_a_registry_package_that_has_a_git_dependency` using per-run pnpr fixture substitution.
- [x] `TypeScript repo: store/pkg-finder/test/readPackageFileMap.test.ts:67` `should resolve git-hosted tarball packages (no type, has tarball)` — write side covered by `tarball_fetcher::tests::writes_index_row_when_writer_provided`; read side reuses the existing tarball-warm prefetch (no git-specific code path).
- [x] `TypeScript repo: store/pkg-finder/test/readPackageFileMap.test.ts:84` `should resolve git dependencies with type "git" and return readable file paths` — same coverage: the write side produces a `gitHostedStoreIndexKey` row at `pkg_id\tbuilt` (see `create_virtual_store::tests::snapshot_cache_key_for_git_resolution_uses_git_hosted_key` for the read-side key shape pin).

Skipped upstream tests to track:

- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:186` `from a non-github git repo` — covered by the local non-host `type: git` path in `install_from_a_git_repo`.
- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:232` `from a github repo that needs to be built. isolated node linker is used` — ported as `git_dependency_is_built_on_isolated_reinstall`.
- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:252` `from a github repo that needs to be built. hoisted node linker is used` — ported as `git_dependency_is_built_on_hoisted_reinstall`.

Rust port notes:

- Frozen install should not resolve git specs, but it must materialize git-hosted package entries from the lockfile.
- Port store/fetcher handling before resolver tests if Stage 1 stays strictly lockfile-driven.

## Lockfile Verification Gate (`minimumReleaseAge`, `trustPolicy`)

The gate ported in pacquet/#11722 re-applies the resolver's policy
checks to every lockfile entry before resolution or fetch, so a
lockfile resolved elsewhere can't reach the install path under a
weaker policy. Spans three new crates
(`pacquet-resolving-resolver-base`, `pacquet-resolving-npm-resolver`,
`pacquet-lockfile-verification`) plus install-side wiring in
`pacquet-package-manager`. Reference: upstream `2a9bd897bf`.

### Trust check (`failIfTrustDowngraded`)

- [x] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:8` `returns "trustedPublisher" when _npmUser.trustedPublisher exists` — `pacquet-resolving-npm-resolver`: implicit in `trust_checks::tests::trusted_publisher_to_provenance_downgrade_fails` (covers the rank assignment).
- [x] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:28` `returns "trustedPublisher" even when attestations.provenance exists` — same coverage as above (the rank function prefers `trustedPublisher`).
- [x] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:53` `returns true when provenance exists` — `trust_checks::tests::provenance_to_unsigned_downgrade_fails` exercises the same rank path.
- [x] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:70` `returns undefined when provenance and attestations are undefined` — covered by `trust_checks::tests::first_version_passes_with_no_history`.
- [x] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:100` `succeeds when no versions have attestation` — `trust_checks::tests::first_version_passes_with_no_history`.
- [x] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:132` `succeeds for version published before first attested version` — `trust_checks::tests::later_publish_does_not_downgrade_earlier_version`.
- [x] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:169` `throws an error when downgrading from provenance to none` — `trust_checks::tests::provenance_to_unsigned_downgrade_fails`.
- [x] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:215` `does not throw an error when only prerelease versions had provenance` — `trust_checks::tests::stable_version_ignores_prerelease_history`.
- [x] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:261` `throws an error when downgrading from trustedPublisher to provenance` — `trust_checks::tests::trusted_publisher_to_provenance_downgrade_fails`.
- [x] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:315` `throws an error when downgrading from trustedPublisher to none` — covered by the same `trusted_publisher_to_provenance_downgrade_fails` plus `provenance_to_unsigned_downgrade_fails` rank logic.
- [x] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:364` `succeeds when maintaining same trust level` — `trust_checks::tests::equal_rank_passes`.
- [x] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:421` `throws an error when version time is missing` — `trust_checks::tests::missing_time_surfaces_trust_check_failed`.
- [x] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:459` `allows downgrade when package@version is in exclude list` — `trust_checks::tests::exclude_exact_version_short_circuits_check`.
- [x] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:501` `allows downgrade when package name is in exclude list (all versions)` — `trust_checks::tests::exclude_any_version_short_circuits_check`.
- [x] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:542` `does not fail with ERR_PNPM_MISSING_TIME when package@version is excluded and time field is missing` — `trust_checks::tests::exclude_exact_version_with_missing_time_does_not_fail`.
- [x] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:564` `does not fail with ERR_PNPM_MISSING_TIME when package name is excluded and time field is missing` — `trust_checks::tests::exclude_package_name_with_missing_time_does_not_fail`.

### Attestation publish-time fetcher

- [x] `TypeScript repo: resolving/npm-resolver/test/fetchAttestationPublishedAt.test.ts:35` `returns an ISO timestamp built from tlogEntries[].integratedTime` — `fetch_attestation_published_at::tests::finds_publish_time_from_single_bundle`.
- [x] `TypeScript repo: resolving/npm-resolver/test/fetchAttestationPublishedAt.test.ts:75` `returns undefined when the registry has no attestations for the package (404)` — `fetch_attestation_published_at::tests::returns_none_on_404`.
- [x] `TypeScript repo: resolving/npm-resolver/test/fetchAttestationPublishedAt.test.ts:86` `returns undefined on 5xx — caller falls back to full metadata` — `fetch_attestation_published_at::tests::returns_none_on_5xx`.
- [x] `TypeScript repo: resolving/npm-resolver/test/fetchAttestationPublishedAt.test.ts:110` `returns undefined when the body is malformed JSON` — `fetch_attestation_published_at::tests::returns_none_on_malformed_body`.
- [x] `TypeScript repo: resolving/npm-resolver/test/fetchAttestationPublishedAt.test.ts:153` `picks the earliest integratedTime across multiple attestations` — `fetch_attestation_published_at::tests::earliest_wins_across_multiple_bundles`.
- [x] `TypeScript repo: resolving/npm-resolver/test/fetchAttestationPublishedAt.test.ts:169` `accepts integratedTime as a number too (defensive against schema drift)` — `fetch_attestation_published_at::tests::accepts_integrated_time_as_number`.
- [x] `TypeScript repo: resolving/npm-resolver/test/fetchAttestationPublishedAt.test.ts:198` `strips a trailing slash on the registry URL` — `fetch_attestation_published_at::tests::trims_trailing_slash_from_registry_root`.

### `createNpmResolutionVerifier`

- [x] `TypeScript repo: resolving/npm-resolver/test/createNpmResolutionVerifier.test.ts:48` `createNpmResolutionVerifier() returns undefined when no policy is active` — `create_npm_resolution_verifier::tests::verifies_tarball_url_when_no_policy_active` / `registry_resolution_with_no_active_policy_skips_metadata_lookup` (plus the `min_age_zero_keeps_age_check_inactive` / `trust_off_keeps_trust_check_inactive` siblings that pin the off-by-one cases).
- [x] `TypeScript repo: resolving/npm-resolver/test/createNpmResolutionVerifier.test.ts:52` `createNpmResolutionVerifier() flags a trustedPublisher → provenance downgrade` — `create_npm_resolution_verifier::tests::trust_downgrade_publisher_to_provenance_fails`.
- [x] `TypeScript repo: resolving/npm-resolver/test/createNpmResolutionVerifier.test.ts:99` `createNpmResolutionVerifier() passes a same-evidence-level version` — `create_npm_resolution_verifier::tests::trust_downgrade_pass_when_no_weaker_evidence`.
- [x] `TypeScript repo: resolving/npm-resolver/test/createNpmResolutionVerifier.test.ts:141` `abbreviated shortcut requires the pinned version to be in metadata` — `create_npm_resolution_verifier::tests::min_age_shortcut_falls_through_when_version_not_listed` (plus the `min_age_pass_via_abbreviated_modified_shortcut` / `min_age_shortcut_falls_through_when_modified_within_cutoff` siblings).
- [x] `TypeScript repo: resolving/npm-resolver/test/createNpmResolutionVerifier.test.ts:187` `ignoreMissingTimeField passes the entry when no source surfaces a timestamp` — `create_npm_resolution_verifier::tests::min_age_missing_time_passes_when_ignored` (plus the fail-closed sibling `min_age_missing_time_fails_closed_by_default`).
- [x] `TypeScript repo: resolving/npm-resolver/test/createNpmResolutionVerifier.test.ts:220` `canTrustPastCheck rejects when the trust-exclude list shrinks` — `create_npm_resolution_verifier::tests::can_trust_past_check_rejects_changed_exclude_list` (covers any shape change, not only shrinkage; matches the stricter upstream contract).

### Cached full-metadata fetcher

- [x] cold cache → 200 → mirror written — `fetch_full_metadata_cached::tests::cold_cache_writes_mirror_on_200`.
- [x] warm cache → 304 → mirror used — `fetch_full_metadata_cached::tests::warm_cache_serves_from_mirror_on_304`.
- [x] stale cache → 200 → mirror overwritten — `fetch_full_metadata_cached::tests::stale_cache_refreshes_mirror_on_200`.
- [x] no cache directory → straight fetch — `fetch_full_metadata_cached::tests::no_cache_dir_skips_mirror_io`.
- [x] read-only cache directory → call still succeeds — `fetch_full_metadata_cached::tests::read_only_cache_dir_does_not_fail_the_call`.

### `verifyLockfileResolutions` runner

- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:35` `no-op when the verifier list is empty` — `verify_lockfile_resolutions::tests::no_verifiers_is_a_noop`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:42` `no-op when lockfile has no packages` — `verify_lockfile_resolutions::tests::no_packages_section_is_a_noop`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:47` `passes when every entry is verified ok` — `verify_lockfile_resolutions::tests::all_ok_emits_started_then_done`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:55` `throws with the verifier-supplied code and reason on a single failure` — `verify_lockfile_resolutions::tests::single_violation_picks_per_policy_variant`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:71` `throws a generic code with per-entry codes in the breakdown when violations span policies` — `verify_lockfile_resolutions::tests::mixed_code_batch_escalates`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:94` `lists violations in stable order across multiple failures` — implicit in `verify_lockfile_resolutions::tests::mixed_code_batch_escalates` (asserts alphabetical name ordering in the breakdown).
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:109` `caps printed violations at 20 with an "…and N more" summary` — `errors::tests::over_cap_adds_and_n_more_summary`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:127` `dedupes peer/patch-suffix variants and invokes the verifier once per (name, version)` — `verify_lockfile_resolutions::tests::one_packages_entry_yields_one_verification`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:183` `keeps the per-policy code when every violation in the batch shares it` — `verify_lockfile_resolutions::tests::single_violation_picks_per_policy_variant`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:202` `runs every active verifier per entry and stops at the first failure` — `verify_lockfile_resolutions::tests::per_candidate_fan_out_stops_at_first_failure`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:232` `skips the verifier when the cache holds an unchanged lockfile + matching policy` — `verify_lockfile_resolutions::tests::second_run_with_cache_skips_fan_out`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:143` `does not collapse same (name, version) with different resolutions` — `verify_lockfile_resolutions::tests::same_name_and_version_with_different_resolutions_are_both_verified`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:166` `the verifier sees the resolution shape verbatim` — `verify_lockfile_resolutions::tests::verifier_receives_the_lockfile_resolution_verbatim`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:264` `does not write a cache record when verification rejects` — `verify_lockfile_resolutions::tests::rejected_verification_does_not_write_a_cache_record`.

### Cache (`tryLockfileVerificationCache`, `recordVerification`)

- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:46` `miss when the cache file does not exist` — `cache::tests::cold_cache_misses_with_populated_stat`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:69` `stat-only hit when size, mtime, and inode all match` — `cache::tests::stat_shortcut_hits_same_path_same_stat`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:132` `miss when a verifier rejects the cached policy` — `cache::tests::policy_invalidation_misses_even_when_stat_matches`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:205` `hit at a new path when the content matches a cached hash (worktree case)` — `cache::tests::content_hash_lookup_finds_same_lockfile_at_different_path`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:256` `malformed lines are ignored, not propagated` — `cache::tests::malformed_lines_are_tolerated_on_read`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:273` `writes a JSONL record with a merged policy bag` — `cache::tests::record_verification_merges_policies`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:341` `appends without rewriting previous lines` — `cache::tests::append_only_log_records_each_call`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:55` `miss when the lockfile path is not in the cache` — `cache::tests::path_without_a_cached_record_misses`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:80` `stat shortcut bails on size mismatch and falls through to hash lookup` — `cache::tests::size_mismatch_falls_through_to_content_hash`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:97` `hash-fallback hit when size matches but mtime/inode were reset` — `cache::tests::changed_stat_falls_through_to_content_hash`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:116` `miss when content changed even if size happens to match` — `cache::tests::changed_same_size_content_misses`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:144` `hit when a verifier accepts the cached policy` — `cache::tests::verifier_can_accept_the_cached_policy`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:176` `hit when every verifier trusts its share of the merged cached policy` — `cache::tests::every_verifier_accepts_its_merged_cached_policy`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:193` `miss when the lockfile no longer exists` — `cache::tests::missing_lockfile_misses_without_hashing`.
- [x] cache compaction past 1.5 MB — `cache::tests::compaction_dedupes_by_path_and_hash`.

### `recordLockfileVerified` wrapper

- [x] `TypeScript repo: installing/deps-installer/test/install/recordLockfileVerified.ts:62` `no-op when cacheDir is undefined` — `record_lockfile_verified` short-circuits on `cache_dir.is_none()`; pinned indirectly via the runner's `second_run_with_cache_skips_fan_out` (which exercises the recorder when caching is on).
- [x] `TypeScript repo: installing/deps-installer/test/install/recordLockfileVerified.ts:72` `no-op when resolutionVerifiers is empty` — same shape as the cache-dir guard.
- [x] `TypeScript repo: installing/deps-installer/test/install/recordLockfileVerified.ts:103` `records the load-equivalent hash — matches what the next install computes off-disk` — `record_lockfile_verified::tests::records_the_hash_read_by_the_next_install`.
- [x] `TypeScript repo: installing/deps-installer/test/install/recordLockfileVerified.ts:141` `respects the caller-supplied lockfilePath` — `record_lockfile_verified::tests::records_the_caller_supplied_lockfile_path`.

### `minimumReleaseAge` install-side behavior

- [x] `TypeScript repo: installing/deps-installer/test/install/minimumReleaseAge.ts:15` `prevents installation of versions that do not meet the required publish date cutoff` — covered end-to-end by `pacquet-package-manager::install::tests::frozen_lockfile_gate_rejects_under_huge_minimum_release_age` and the CLI integration test `cli::lockfile_verification::install_fails_under_huge_minimum_release_age`.
- [x] `TypeScript repo: installing/deps-installer/test/install/minimumReleaseAge.ts:23` `ignored for packages in the minimumReleaseAgeExclude array` — `create_npm_resolution_verifier::tests::verify_skips_age_check_when_package_excluded`.
- [x] `TypeScript repo: installing/deps-installer/test/install/minimumReleaseAge.ts:128` `throws error when semver range is used in minimumReleaseAgeExclude` — `pacquet-package-manager::install::tests::install_rejects_invalid_minimum_release_age_exclude_pattern`.
- [x] `TypeScript repo: installing/deps-installer/test/install/minimumReleaseAge.ts:32` `ignored using a pattern` — `create_npm_resolution_verifier::tests::verify_skips_age_check_when_package_matches_exclude_pattern`.
- [x] `TypeScript repo: installing/deps-installer/test/install/minimumReleaseAge.ts:41` `ignored for specific exact versions in minimumReleaseAgeExclude` — `create_npm_resolution_verifier::tests::verify_skips_age_check_for_an_exact_version_in_a_union`.
- [x] `TypeScript repo: installing/deps-installer/test/install/minimumReleaseAge.ts:68` `falls back to immature version when no mature version satisfies the range (non-strict mode)` — ported as `pacquet-cli::lockfile_verification::non_strict_minimum_release_age_falls_back_when_no_mature_version_matches`: the non-strict install falls back to the lowest matching immature version end to end.
- [x] `TypeScript repo: installing/deps-installer/test/install/minimumReleaseAge.ts:86` `strict minimumReleaseAge surfaces every immature pick via handleResolutionPolicyViolations, then aborts` — `minimum_release_age::tests::non_interactive_strict_mode_reports_every_immature_pick`.
- [x] `TypeScript repo: installing/deps-installer/test/install/minimumReleaseAge.ts:140` `enforced on an existing lockfile entry that does not meet the cutoff` — `pacquet-cli::lockfile_verification::install_fails_under_huge_minimum_release_age` starts from a lockfile entry and pins the gate.

### Version-policy parser

- [x] `TypeScript repo: config/version-policy/test/index.ts:8` `createPackageVersionPolicy()` — `pacquet-config::version_policy::tests` exhaustive coverage of the parsing + matcher contract.
- [x] `TypeScript repo: config/version-policy/test/index.ts:57` `createPackageVersionPolicyOrThrow() rewraps parser errors with INVALID_<KEY>` — handled at the install boundary in `pacquet-package-manager::build_resolution_verifiers` (wraps `VersionPolicyError` → `BuildVerifiersError::InvalidMinimumReleaseAgeExclude` / `InvalidTrustPolicyExclude`).

Rust port notes:

- The abbreviated-modified shortcut has landed (`try_abbreviated_modified_shortcut` / `fetch_abbreviated_meta` in `create_npm_resolution_verifier.rs`, pinned by the `min_age_*_shortcut_*` tests).
- The cache cross-stack contract is content-divergent on hash format only — pacquet writes sha256-**hex** where pnpm writes sha256-**base64** (object-hash's default). Each stack reads its own records out of the shared JSONL; cross-stack hits aren't expected and aren't tested.
- The end-to-end CLI test uses a 100-year `minimumReleaseAge` to sidestep the mocked registry's real-world `time` field. A finer-grained fixture with controlled `time` values lives in the unit tests (`fetch_full_metadata_cached::tests`, `create_npm_resolution_verifier::tests`).

## `optimisticRepeatInstall` + `checkDepsStatus` Pre-Install Shortcut

Tracks pnpm/pnpm#11940. Pacquet's port (`pacquet-package-manager::optimistic_repeat_install`) covers the mtime-vs-`lastValidatedTimestamp` branch of upstream's `checkDepsStatus`. Ported tests live in `optimistic_repeat_install::tests` and the install-level `optimistic_repeat_install_skips_entire_pipeline_when_state_is_fresh` end-to-end.

### Ported

- [x] `TypeScript repo: deps/status/test/checkDepsStatus.test.ts:55` `returns upToDate: false when overrides have changed` — `optimistic_repeat_install::tests::returns_skipped_when_overrides_drift`.
- [x] `TypeScript repo: deps/status/test/checkDepsStatus.test.ts:115` `returns upToDate: false when ignoredOptionalDependencies have changed` — `optimistic_repeat_install::tests::returns_skipped_when_ignored_optional_dependencies_drift`.
- [x] `TypeScript repo: deps/status/test/checkDepsStatus.test.ts:145` `returns upToDate: false when patchedDependencies have changed` — `optimistic_repeat_install::tests::returns_skipped_when_patched_dependencies_drift`.
- [x] `TypeScript repo: deps/status/test/checkDepsStatus.test.ts:205` `returns upToDate: false when allowBuilds have changed` — `optimistic_repeat_install::tests::returns_skipped_when_allow_builds_drift`.
- [x] `TypeScript repo: deps/status/test/checkDepsStatus.test.ts:85` `returns upToDate: false when packageExtensions have changed` — split out as `optimistic_repeat_install::tests::returns_skipped_when_package_extensions_drift` once the yaml field landed in `Config`.
- [x] `TypeScript repo: deps/status/test/checkDepsStatus.test.ts:175` `returns upToDate: false when peersSuffixMaxLength has changed` — `optimistic_repeat_install::tests::returns_skipped_when_peers_suffix_max_length_drift` (`peersSuffixMaxLength` now lives in `Config` and drift-checks like the other settings).
- [x] `TypeScript repo: deps/status/test/checkDepsStatus.test.ts:328` `returns upToDate: false when a patch was modified and manifests were not modified` — `optimistic_repeat_install::tests::returns_skipped_when_patch_file_modified_after_validation` (the patch-mtime branch of `patchesOrHooksAreModified`; companion `returns_up_to_date_when_patch_file_unchanged`).

### Remaining upstream coverage

- [x] `TypeScript repo: deps/status/test/checkDepsStatus.test.ts:234` `skips the allowBuilds change detection when allowBuilds is in ignoredWorkspaceStateSettings` — `optimistic_repeat_install::tests::returns_skipped_when_allow_builds_drift` also calls `check_optimistic_repeat_install_ignoring` and pins the ignored-key result.
- [x] `TypeScript repo: deps/status/test/checkDepsStatus.test.ts:270` `returns upToDate: false when a pnpmfile was modified` — `optimistic_repeat_install::tests::returns_skipped_when_a_pnpmfile_is_modified`.
- [x] `TypeScript repo: deps/status/test/checkDepsStatus.test.ts:405` `returns upToDate: false when the wanted lockfile has merge conflict markers` and `:438` `returns upToDate: false when a project lockfile has merge conflict markers and sharedWorkspaceLockfile is false` — `optimistic_repeat_install::tests::returns_skipped_when_wanted_lockfile_has_merge_conflict_markers` and `returns_skipped_when_project_lockfile_has_merge_conflict_markers`.

## `catalogMode` Auto-Cataloging (`saveCatalogName` / catalog write-back)

Tracks pnpm/pnpm#12196. The `catalogMode` mismatch gate landed earlier (pnpm#11706); this is the auto-cataloging half — writing `catalog:` / `catalog:<name>` to `package.json`, the entry to `pnpm-workspace.yaml`, and the snapshot to `pnpm-lock.yaml`. The decision core is `pacquet-package-manager::catalog_mode::decide_catalog`; the format-preserving workspace writer is the `pacquet-workspace-manifest-writer` crate; the lockfile `catalogs:` snapshot is built in `dependencies_graph_to_lockfile::build_catalog_snapshots`.

### Ported

- [x] `TypeScript repo: installing/deps-installer/test/catalogs.ts:1312` `adding with catalogMode: strict will add to or use from catalog` — `pacquet-cli::catalog::add_strict_catalogs_a_new_dependency`.
- [x] `TypeScript repo: installing/deps-installer/test/catalogs.ts:1348` `re-adding existing catalog dependency with catalogMode: strict preserves catalog specifier` (pnpm#10176) — `pacquet-cli::catalog::readd_catalog_dependency_preserves_specifier`.
- [x] `TypeScript repo: installing/deps-installer/test/catalogs.ts:1404` `adding with catalogMode: prefer will add to or use from catalog` — `pacquet-cli::catalog::add_prefer_catalogs_a_new_dependency`.
- [x] `TypeScript repo: installing/deps-installer/test/catalogs.ts:1435` `adding mismatched version with catalogMode: strict will error` — `pacquet-cli::catalog::add_mismatched_version_strict_errors`.
- [x] `TypeScript repo: installing/deps-installer/test/catalogs.ts:1840` `update --latest works on named catalog dependency with catalogMode=prefer` — `pacquet-cli::catalog::update_latest_named_catalog_bumps_the_entry`.
- [x] `TypeScript repo: workspace/workspace-manifest-writer/test/addCatalogs.test.ts` and `updateWorkspaceManifest.test.ts` (catalog cases) — ported as byte-for-byte unit tests in `pacquet-workspace-manifest-writer::tests` (comment/blank-line/quote-style/sorted-insert/named-catalog preservation).
- [x] Decision-core branches (gate strict/prefer/manual, named-catalog resolution, `--save-catalog-name`, runtime skip) — `pacquet-package-manager::catalog_mode::tests`.
- [x] `TypeScript repo: installing/deps-installer/test/catalogs.ts:789` `catalog entry using npm alias can be reused` — the `catalogs:` snapshot assertion (`{ specifier: npm:…, version: … }` for an `npm:`-aliased catalog entry) is ported as `dependencies_graph_to_lockfile::tests::aliased_catalog_dependency_records_catalog_snapshot`. The reuse half is covered at the single-importer level by `catalog_mode::tests::reinstalling_a_catalog_dependency_reuses_the_existing_entry` (aliased reuse verified stable manually); the test's two-project shape needs workspaces (pacquet/pacquet#431).

### Not yet ported / known divergences

- [x] `TypeScript repo: installing/commands/test/saveCatalog.ts` — `pacquet-cli::catalog::save_catalog_flag_writes_the_default_catalog` and `save_catalog_name_preserves_the_dependency_group` cover the command-level default/named and `--save-dev` flows.
- [x] `TypeScript repo: installing/deps-installer/test/catalogs.ts` general integration cases — `pacquet-cli::catalog::install_with_catalog_reference_writes_catalog_snapshot` pins install resolution and the lockfile snapshot; the existing catalog unit and workspace tests cover filtering and pruning at their implementation boundaries.
- [x] `cleanupUnusedCatalogs` — implemented end to end: `pacquet-cli::catalog::removes_unused_entries_from_the_workspace_catalog` covers the CLI flow, and `pacquet-workspace-manifest-writer::tests::remove_unused_catalogs` ports the `removeCatalogs.test.ts` suite.
- [x] Manual-mode `update --latest` of a `catalog:` dependency — `pacquet-cli::catalog::update_latest_keeps_catalog_reference_in_manual_mode`.

### Rust port notes

- Settings drift comparison is field-by-field on `WorkspaceStateSettings::PartialEq` rather than the upstream `Object.entries` walk. Equivalent in behavior: any field present in the cached state but `None` in today's `current_settings` (or vice versa) trips the check.
- The settings construction is shared between the writer (`build_workspace_state` in `install.rs`) and the reader (`current_settings` in `optimistic_repeat_install.rs`) so adding a tracked field on one side automatically updates the other.

## Peer Resolution (`installing/deps-resolver/test/resolvePeers.ts`, `hoistPeers.test.ts`)

Status of the upstream peer-resolution suites, audited while landing the
lockfile-parity peer fixes (pnpm/pnpm#12266, pnpm/pnpm#12267).

### `hoistPeers.test.ts` — fully ported

- [x] All 11 cases (`hoistPeers` × 8 + `getHoistableOptionalPeers` × 3) are ported in `hoist_peers/tests.rs`, plus two prerelease siblings pacquet adds.

### `resolvePeers.ts` — ported / covered

- [x] `transitive peers use version-only suffixes` — `dedupe_peers_propagates_transitive_peer_to_parent`.
- [x] `uses version-only peer suffixes without nested dep paths` — `dedupe_peers_collapses_nested_peer_suffixes` / `no_dedupe_peers_keeps_nested_peer_suffixes`.
- [x] `resolve peer dependencies of cyclic dependencies` — `cyclic_peer_dependencies_resolve_cleanly`.
- [x] `when a package is referenced twice … still try to resolve it in the other occurrence` — `revisit_resolves_peer_in_one_occurrence_misses_in_other`.
- [x] `should return from where the bad peer dependency is resolved` — `bad_peer_inside_subtree_records_resolved_from_parent`.
- [x] `should find peer dependency conflicts` — covered by `bad_peer_version_is_reported`.
- [x] `a peer's own peer is shared with a sibling that peer-depends both` — ported as `peers_own_peer_shared_with_sibling_that_peer_depends_both`.
- [x] `transitive pending peer uses provider final suffix` — ported as `transitive_pending_peer_uses_provider_final_suffix`.
- [x] Walk-ancestor suffix propagation (no direct upstream case — pacquet-specific manifestation of the deferred `calculateDepPath`) — `ancestor_peer_carries_its_own_suffix`.
- [x] Optional peer not hoisted from the run-resolved tree (resolveRootDependencies behavior behind `getHoistableOptionalPeers`) — `optional_peer_only_in_resolved_tree_is_not_hoisted`.
- [x] `build_final_graph` min-depth tie-break across `pure_pkgs`/`find_hit` revisits (pacquet-specific) — `shallower_pure_pkgs_revisit_lowers_graph_depth`.

### High-level install / CLI peer lockfile coverage

- [x] `TypeScript repo: installing/deps-installer/test/install/peerDependencies.ts` `transitive pending peer uses provider final suffix in lockfile` — package-manager port added as `fresh_install_uses_final_peer_suffix_for_transitive_pending_peer`.
- [x] `TypeScript repo: pnpm/test/install/peerDependencies.ts` `transitive pending peer uses provider final suffix in lockfile` — CLI port added as `transitive_pending_peer_uses_provider_final_suffix_in_lockfile`.

### `resolvePeers.ts` — not yet ported

- [x] `multi-project: different peer versions produce different instances` — `resolve_peers::tests::workspace_importers_get_distinct_instances_for_different_peer_versions`.
- [x] `resolve peer dependencies with npm aliases` — `resolve_peers::tests::alias_child_resolves_peer_by_real_package_name` and `own_peer_is_resolved_from_aliased_sibling_real_name` pin alias-based peer suffix resolution.
- [x] `should find peer dependency conflicts when the peer is an optional peer of one of the dependencies`, `should ignore conflicts between missing optional peer dependencies`, `should pick the single wanted peer dependency range`, `should return the intersection of two compatible ranges`, the two prerelease-warning cases — the implemented optional-conflict path is pinned by `resolve_peers::tests::reports_a_conflict_for_an_optional_peer_with_an_incompatible_provider`; the compatible-range intersection is ported as `resolve_importer::tests::auto_installed_peer_uses_the_intersection_of_compatible_ranges` (`merge_ranges` now intersects distinct compatible ranges via `node-semver`'s `Range::intersect`), while the existing prerelease and missing-optional tests pin those implemented branches.
- [x] The `lockedPeerContext` / `resolvedPeerProviderPaths` series — `resolve_peers` carries `locked_peer_context` / `previous_dep_path` tree-node fields, a `resolved_peer_provider_paths` option, and a `paths_by_node_id` output; ported as `resolve_peers::tests::locked_peer_provider_preferences::{compatible_locked_peer_provider_is_reused, locked_peer_provider_outside_the_current_range_is_not_reused}`. The install-path wiring (capturing the locked context from the wanted lockfile and running the second pass) is a follow-up; the must-win sibling cases port with it.

## `pnpm logout` (`@pnpm/auth.commands`)

Pacquet's port lives in `pacquet-auth-commands` (`logout` module) with the CLI adapter in `pacquet-cli`'s `cli_args::logout`. Upstream injects its side effects (`fetch`, `readIniFile`, `writeIniFile`, `globalInfo`, `globalWarn`) through a `LogoutContext` object of functions; the Rust port threads them through the project's capability-trait seam instead (`FsReadToString` / `FsWrite` / `RevokeToken` on `Sys`, plus `R: Reporter` for the two `global*` channels). The whole upstream suite translates, so every case is a unit test of the ported `logout` function with unit-struct fakes.

### Ported

- [x] `TypeScript repo: pnpm11/auth/commands/test/logout.test.ts:41` `should throw when not logged in` — `logout::tests::throws_when_not_logged_in`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/logout.test.ts:53` `should throw when not logged in to a custom registry` — `logout::tests::throws_when_not_logged_in_to_a_custom_registry`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/logout.test.ts:66` `should revoke token on registry and remove from auth.ini` — `logout::tests::revokes_token_on_registry_and_removes_from_auth_ini`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/logout.test.ts:106` `should logout from a custom registry` — `logout::tests::logs_out_from_a_custom_registry`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/logout.test.ts:142` `should still remove token locally when registry returns non-ok response` — `logout::tests::removes_token_locally_when_registry_returns_non_ok`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/logout.test.ts:172` `should still remove token locally when fetch throws a network error` — `logout::tests::removes_token_locally_when_fetch_errors`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/logout.test.ts:204` `should warn when token is not in auth.ini (e.g. from .npmrc)` — `logout::tests::warns_when_token_is_not_in_auth_ini`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/logout.test.ts:236` `should throw when registry call fails and token is not in auth.ini` — `logout::tests::throws_when_registry_call_fails_and_token_not_in_auth_ini`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/logout.test.ts:270` `should warn when auth.ini does not exist (ENOENT) and token comes from another source` — `logout::tests::warns_when_auth_ini_does_not_exist`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/logout.test.ts:301` `should propagate non-ENOENT errors from readIniFile` — `logout::tests::propagates_non_not_found_read_errors`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/logout.test.ts:322` `should URL-encode the token when revoking` — `logout::tests::url_encodes_the_token_when_revoking`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/logout.test.ts:349` `should normalize the registry URL` — `logout::tests::normalizes_the_registry_url`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/logout.test.ts:377` `should handle registry with a path` — `logout::tests::handles_registry_with_a_path`.

## `pnpm login` / `pnpm adduser` (`@pnpm/auth.commands`)

Pacquet's port lives in `pacquet-auth-commands` (`login` module) with the CLI adapter in `pacquet-cli`'s `cli_args::login`. Upstream injects its side effects through a `LoginContext` object of functions (`Date`, `setTimeout`, `createReadlineInterface`, `enquirer`, `fetch`, `globalInfo`/`globalWarn`, `process`, `readIniFile`/`writeIniFile`); the Rust port threads them through the project's seams instead. The interactive OTP / web-auth effects reuse `pacquet-network-web-auth`'s eight capability traits, the credential prompts are the crate-local `PromptInput` / `PromptPassword` capabilities, `auth.ini` I/O reuses logout's `FsReadToString` / `FsWrite`, and the two `global*` channels flow through `R: Reporter`. The web-login `POST` and classic `PUT` go over the shared `ThrottledClient` against a `mockito` server (the real-fixture route), so only the effects a fixture can't stage portably sit behind the `Sys` seam. The whole upstream suite translates.

### Ported

- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:86` `should throw in non-interactive terminal` — `login::tests::should_throw_in_non_interactive_terminal`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:98` `should use web login when registry supports it` — `login::tests::should_use_web_login_when_registry_supports_it`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:146` `should persist a scoped auth token and scope registry mapping when --scope is provided` — `login::tests::should_persist_a_scoped_auth_token_and_scope_registry_mapping`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:185` `should persist scoped auth tokens under path registries` — `login::tests::should_persist_scoped_auth_tokens_under_path_registries`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:224` `should accept --scope with a leading @ and not double-prefix` — `login::tests::should_accept_scope_with_a_leading_at_and_not_double_prefix`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:254` `should not write a scope mapping when --scope is omitted` — `login::tests::should_not_write_a_scope_mapping_when_scope_is_omitted`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:284` `should fall back to classic login when web login returns 404` — `login::tests::should_fall_back_to_classic_login_when_web_login_returns_404`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:338` `should handle classic OTP challenge during login` — `login::tests::should_handle_classic_otp_challenge_during_login`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:393` `should handle webauth OTP challenge during login` — `login::tests::should_handle_webauth_otp_challenge_during_login`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:459` `should not trigger OTP for non-401 errors` — `login::tests::should_not_trigger_otp_for_non_401_errors`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:496` `should throw when username is empty in classic login` — `login::tests::should_throw_when_username_is_empty_in_classic_login`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:528` `should throw when classic login returns no token` — `login::tests::should_throw_when_classic_login_returns_no_token`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:567` `should throw when web login returns invalid response (missing loginUrl/doneUrl)` — `login::tests::should_throw_when_web_login_returns_invalid_response`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:588` `should fall back to classic login when web login returns 405` — `login::tests::should_fall_back_to_classic_login_when_web_login_returns_405`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:635` `should not trigger OTP for 401 without www-authenticate otp header` — `login::tests::should_not_trigger_otp_for_401_without_www_authenticate_otp_header`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:673` `should succeed when config file does not exist (ENOENT)` — `login::tests::should_succeed_when_config_file_does_not_exist`.
- [x] `TypeScript repo: pnpm11/auth/commands/test/login.test.ts:711` `should propagate non-ENOENT errors from readIniFile` — `login::tests::should_propagate_non_enoent_errors_from_reading_auth_ini`.
