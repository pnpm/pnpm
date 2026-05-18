# Test Porting Plan

Scope: Stage 1 from pnpm/pacquet#299. The target behavior is `pacquet install --frozen-lockfile` matching `pnpm install --frozen-lockfile`. The TypeScript repo is the source of truth. This Rust repo should port these tests before or alongside the corresponding behavior.

Unless otherwise noted, TypeScript paths are relative to the TypeScript repo root, and Rust commands are run from this Rust repo root. Line numbers point at the TypeScript tests as found during this audit. Some tests are not frozen-lockfile tests themselves, but they exercise shared code or invariants that the frozen/headless installer must preserve.

**Do not stop at the per-package `test/` directories.** Each TypeScript package has its own unit and integration tests under directories such as `installing/deps-installer/test/`, `installing/deps-restorer/test/`, `network/auth-header/test/`, and `config/reader/test/`. In addition to those, the upstream repo has a top-level **`pnpm/test/`** directory. That directory contains CLI-level, end-to-end integration tests for almost every feature in pnpm, including install, monorepo and workspaces, lifecycle scripts, lockfile, global virtual store, runtime install, publish, link, store, prune, audit, dedupe, exec, and run. These tests drive the real `pnpm` CLI binary and exercise behaviors that the per-package tests do not, such as end-to-end config resolution, exit codes, stdout and stderr formatting, and multi-step CLI flows. When porting any feature, you must:

1. Look in the per-package `test/` directory for that feature's package, **and**
2. Look in `pnpm/test/` for matching CLI-level coverage (e.g. `pnpm/test/install/`, `pnpm/test/monorepo/`, `pnpm/test/install/lifecycleScripts.ts`, `pnpm/test/install/globalVirtualStore.ts`, `pnpm/test/install/runtimeOnFail.ts`, etc.).

This plan already cites a handful of `pnpm/test/...` files inline next to the feature they belong to, but those citations are not exhaustive. Treat `pnpm/test/` as a parallel test tree that must be audited for every feature you port, not just the ones it is already mentioned under. Skipping `pnpm/test/` is the single most common way a port misses behavioral coverage.

Expected-failing test ports should live under a `known_failures` test module and use `pacquet_testing_utils::allow_known_failure!` at the not-yet-implemented subject-under-test boundary. List all expected failures with `just known-failures`.

Test the tests before marking them ported. After porting a test, temporarily modify the relevant implementation path so the test should fail, run that test, and verify it fails for the expected reason. Revert the temporary breakage before committing. This guards against porting tests that execute but do not actually detect the behavior they claim to cover. See https://github.com/pnpm/pacquet/issues/299#issuecomment-4323032648.

Having more tests than pnpm is a plus, but it is not strictly required. The lists in this plan are a floor, not a ceiling. Porting the upstream coverage is the minimum bar for behavioral parity. Beyond that minimum, pacquet-only tests that exercise edge cases, regressions, or invariants the upstream suite does not cover are welcome and encouraged, but contributors are not obligated to add them. Do not hold back extra coverage just to keep the two suites symmetric.

## `.modules.yaml` Write And Verify

Primary tests:

- [x] `TypeScript repo: installing/modules-yaml/test/index.ts:10` `writeModulesManifest() and readModulesManifest()`
- [x] `TypeScript repo: installing/modules-yaml/test/index.ts:42` `backward compatible read of .modules.yaml created with shamefully-hoist=true`
- [x] `TypeScript repo: installing/modules-yaml/test/index.ts:55` `backward compatible read of .modules.yaml created with shamefully-hoist=false`
- [x] `TypeScript repo: installing/modules-yaml/test/index.ts:68` `readModulesManifest() should create a node_modules directory`
- [x] `TypeScript repo: installing/modules-yaml/test/index.ts:96` `readModulesManifest does not fail on empty file`

Frozen/headless install coverage:

- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:54` `installing a simple project` verifies headless install writes a modules manifest.
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:819` `installing with no symlinks but with PnP` verifies `.modules.yaml` still exists when symlinks are disabled.
- [ ] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:24` `should hoist dependencies` verifies `hoistedDependencies` is preserved on repeat frozen install.
- [ ] `TypeScript repo: installing/deps-installer/test/install/modulesCache.ts:52` `the modules cache is pruned when it expires and headless install is used` verifies `prunedAt` is read, rewritten, and honored by headless install.
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:74` `skip optional dependency that does not support the current OS` verifies `skipped` survives frozen reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/lockfile.ts:614` `pendingBuilds gets updated if install removes packages` verifies `.modules.yaml.pendingBuilds` is rewritten after pruning.
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:205` `GVS re-links when allowBuilds changes` verifies GVS-related `allowBuilds` state is updated in `.modules.yaml`.
- [ ] `TypeScript repo: pnpm/test/monorepo/index.ts:1467` `custom virtual store directory in a workspace with not shared lockfile` verifies frozen reinstall preserves custom `virtualStoreDir` serialization.
- [ ] `TypeScript repo: pnpm/test/monorepo/index.ts:1514` `custom virtual store directory in a workspace with shared lockfile` verifies frozen reinstall preserves root `virtualStoreDir` serialization.

Rust port notes:

- Start with pure read/write tests before install tests.
- Copy the TypeScript legacy fixtures from `TypeScript repo: installing/modules-yaml/test/fixtures/`.
- Assert raw serialized `virtualStoreDir` where the TS test checks serialization, not normalized path resolution.

## Proper Support Of `optionalDependencies`

Primary frozen/headless tests:

- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:74` `skip optional dependency that does not support the current OS` removes `node_modules`, reinstalls with `frozenLockfile: true`, and verifies skipped packages remain skipped.
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:283` `optional subdependency is skipped` includes forced headless install with `force: true, frozenLockfile: true` and verifies incompatible optional subdependency handling.
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:359` `only that package is skipped which is an optional dependency only and not installable` removes `node_modules`, reinstalls frozen, and guards optional/non-optional overlap.
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:594` `install optional dependency for the supported architecture set by the user (nodeLinker=%s)` includes `nodeLinker` variants and frozen reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:665` `optional dependency is hardlinked to the store if it does not require a build` includes frozen reinstall and import-method parity.
- [ ] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:540` `hoisting should not create a broken symlink to a skipped optional dependency` covers public hoist plus skipped optional dependency in headless behavior.

Supporting tests:

- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:300` `installing only optional deps` covers headless include filtering when only optional dependencies are selected.
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:323` `not installing optional deps` covers headless include filtering.
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:340` `skipping optional dependency if it cannot be fetched` verifies a failed optional fetch does not fail headless install and still writes install state.
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:21` `successfully install optional dependency with subdependencies`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:27` `skip failing optional dependencies`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:34` `skip failing optional peer dependencies`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:45` `skip non-existing optional dependency`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:143` `skip optional dependency that does not support the current Node version`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:169` `do not skip optional dependency that does not support the current pnpm version`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:199` `don't skip optional dependency that does not support the current OS when forcing`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:213` `optional subdependency is not removed from current lockfile when new dependency added`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:344` `optional subdependency of newly added optional dependency is skipped`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:391` `not installing optional dependencies when optional is false`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:419` `optional dependency has bigger priority than regular dependency`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:436` `only skip optional dependencies`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:470` `skip optional dependency that does not support the current OS, when doing install on a subset of workspace projects`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:540` `do not fail on unsupported dependency of optional dependency`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:552` `fail on unsupported dependency of optional dependency`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:563` `do not fail on an optional dependency that has a non-optional dependency with a failing postinstall script`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:574` `fail on a package with failing postinstall if the package is both an optional and non-optional dependency`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:618` `remove optional dependencies that are not used`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:633` `remove optional dependencies that are not used, when hoisted node linker is used`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:648` `remove optional dependencies if supported architectures have changed and a new dependency is added`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:703` `complex scenario with same optional dependencies appearing in many places of the dependency graph`
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:712` `dependency that is both optional and non-optional is installed, when optional dependencies should be skipped`
- [ ] `TypeScript repo: resolving/npm-resolver/test/optionalDependencies.test.ts:27` `optional dependencies receive full metadata with libc field` ensures optional dependency metadata includes platform/libc fields.
- [ ] `TypeScript repo: resolving/npm-resolver/test/optionalDependencies.test.ts:73` `abbreviated and full metadata are cached separately` prevents regular dependency metadata cache from hiding optional metadata.
- [ ] `TypeScript repo: installing/package-requester/test/index.ts:852` `do not fetch an optional package that is not installable` covers cold-store requester behavior for unsupported optional packages.
- [ ] `TypeScript repo: installing/package-requester/test/index.ts:1205` `should pass optional flag to resolve function` ensures resolver receives `optional: true`.

Rust port notes:

- Separate platform/architecture skip semantics from the generic optional dependency group filtering.
- These tests depend on `.modules.yaml.skipped`, so port that field first.

## Hoisting (`hoistPattern`, `publicHoistPattern`)

Primary tests:

- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:24` `should hoist dependencies` repeats install with `hoistPattern: '*'` and `frozenLockfile: true`. Single-importer subset ported as `private_hoist_default_pattern_hoists_transitives` in `crates/cli/tests/hoist.rs`. Repeat-install map preservation lives in the `known_failures` module (blocked on partial install, pnpm/pacquet#433).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:53` `should hoist dependencies to the root of node_modules when publicHoistPattern is used` covers baseline public hoist behavior. Ported as `public_hoist_star_hoists_to_root_node_modules`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:71` `public hoist should not override directories that are already in the root of node_modules`. Stubbed in `known_failures::public_hoist_preserves_existing_root_directories` — pacquet's `symlink_package` does the conservative EEXIST swallow but not upstream's external-symlink introspection.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:89` `should hoist some dependencies to the root of node_modules when publicHoistPattern is used and others to the virtual store directory` covers combined private and public hoist patterns. Stubbed in `known_failures::combined_public_and_private_hoist_patterns_split_targets` — pacquet's algo handles it (covered by `public_pattern_wins_ties` unit test) but the upstream test uses package set the registry mock doesn't carry.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:107` `should hoist dependencies by pattern` covers pattern-specific private hoisting. Ported as `private_hoist_pattern_filters_aliases`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:121` `should remove hoisted dependencies`. Stubbed in `known_failures::should_remove_hoisted_dependencies` (partial install, #433).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:137` `should not override root packages with hoisted dependencies`. Stubbed in `known_failures::should_not_override_root_packages_with_hoisted_deps` (#433).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:148` `should rehoist when uninstalling a package`. Stubbed (#433).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:169` `should rehoist after running a general install`. Stubbed (#433).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:201` `should not override aliased dependencies`. Stubbed (#433 + alias-aware install plumbing).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:209` `hoistPattern=* throws exception when executed on node_modules installed w/o the option`. Stubbed (#433 — pattern-change detection across `.modules.yaml` reads).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:220` `hoistPattern=undefined throws exception when executed on node_modules installed with hoist-pattern=*`. Stubbed (#433).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:233` `hoist by alias`. Stubbed in `known_failures::hoist_by_alias` — algo is correct (unit-tested) but end-to-end exercises alias plumbing not all wired.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:249` `should remove aliased hoisted dependencies`. Stubbed (#433).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:272` `should update .modules.yaml when pruning if we are flattening`. Stubbed (#433).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:288` `should rehoist after pruning`. Stubbed (#433).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:320` `should hoist correctly peer dependencies`. Stubbed in `known_failures::should_hoist_correctly_peer_dependencies` — multi-variant peer install path not exercised.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:327` `should uninstall correctly peer dependencies`. Stubbed (#433).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:341` `hoist-pattern: hoist all dependencies to the virtual store node_modules` covers workspace install followed by frozen reinstall. Basic workspace shape ported as `workspace_hoist_walks_every_importer`; the upstream test additionally asserts preservation across re-installs which still needs partial install (#433) — stubbed in `known_failures::workspace_hoist_all_to_virtual_store_node_modules`.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:423` `hoist when updating in one of the workspace projects`. Stubbed in `known_failures::workspace_hoist_when_updating_one_project` — needs `pnpm add`-equivalent manifest mutation.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:514` `should recreate node_modules with hoisting`. Stubbed (#433).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:540` `hoisting should not create a broken symlink to a skipped optional dependency` covers hoisting with skipped optional packages. Stubbed in `known_failures::hoisting_skips_broken_symlink_for_skipped_optional` — pacquet doesn't yet skip optional deps on platform constraints.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:567` `the hoisted packages should not override the bin files of the direct dependencies` covers public hoist bin precedence after frozen reinstall. Stubbed in `known_failures::hoisted_packages_dont_override_direct_dep_bins` — bin-conflict resolution rules not implemented.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:587` `hoist packages which is in the dependencies tree of the selected projects`. Stubbed in `known_failures::workspace_hoist_packages_in_selected_projects_tree` — needs `--filter` selected-projects install, which workspace install (#443) didn't implement.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:682` `only hoist packages which is in the dependencies tree of the selected projects with sub dependencies`. Stubbed (`--filter` selected-projects install).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:790` `should add extra node paths to command shims`. Stubbed in `known_failures::should_add_extra_node_paths_to_command_shims` — `extendNodePath` not implemented.
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:799` `should not add extra node paths to command shims, when extend-node-path is set to false`. Stubbed (extendNodePath).
- [x] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:813` `hoistWorkspacePackages should hoist all workspace projects` covers workspace package hoisting and frozen reinstall. Stubbed in `known_failures::hoist_workspace_packages_hoists_all_workspace_projects` — needs the `hoistedWorkspacePackages` shape pacquet doesn't model yet (links workspace projects themselves into the hoist tree).

Headless module-manifest checks:

- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:569` `installing with hoistPattern=*` asserts private `hoistedDependencies` in `.modules.yaml`. Ported as `modules_yaml_records_hoisted_dependencies` and `private_hoist_links_bins` in `crates/cli/tests/hoist.rs`.
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:628` `installing with publicHoistPattern=*` asserts public `hoistedDependencies` in `.modules.yaml`. Ported as `public_hoist_star_hoists_to_root_node_modules` and `public_hoist_bin_is_linked_via_root_bin_dir`.
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:690` `installing with publicHoistPattern=* in a project with external lockfile` covers headless public hoist with an external lockfile/project root split.
- [ ] `TypeScript repo: installing/deps-installer/test/install/sideEffects.ts:50` `caching side effects of native package when hoisting is used` is skipped upstream but documents side-effects cache behavior under private hoisting.

Rust port notes:

- Port the module-manifest assertions with hoisting; otherwise later behavior can appear to work while install state is wrong.

## Support `patchedDependencies`

Primary frozen/headless tests:

- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:24` `patch package with exact version` verifies patched package lockfile/snapshot/side-effects, then frozen reinstall and frozen hoisted reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:120` `patch package with version range` covers range selector patches with frozen and hoisted frozen reinstalls.
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:297` `patch package when scripts are ignored` covers patches with ignored scripts and frozen reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:386` `patch package when the package is not in allowBuilds list` verifies patches apply even when builds are disallowed, including frozen and hoisted frozen paths.

Supporting tests:

- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:848` `installing with no modules directory and a patched dependency` covers headless patched dependency behavior when `enableModulesDir: false`.
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:216` `patch package reports warning if not all patches are applied and allowUnusedPatches is set`
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:246` `patch package throws an exception if not all patches are applied`
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:269` `the patched package is updated if the patch is modified`
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:475` `patch package when the patched package has no dependencies and appears multiple times`
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:508` `patch package should fail when the exact version patch fails to apply`
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:530` `patch package should fail when the version range patch fails to apply`
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:552` `patch package should fail when the name-only range patch fails to apply`

Rust port notes:

- The primary tests are enough for the frozen installer milestone.
- The supporting tests belong with patch parser/application correctness and should be ported when Rust has a patching subsystem.

## Support Building Dependencies

Primary frozen/headless tests:

- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:331` `lifecycle scripts run before linking bins` removes `node_modules`, reinstalls frozen, and verifies generated bins are executable.
- [ ] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:351` `hoisting does not fail on commands that will be created by lifecycle scripts on a later stage` covers `hoistPattern: '*'` and frozen install.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:372` `bins are linked even if lifecycle scripts are ignored` verifies bin linking after frozen reinstall with ignored scripts.
- [ ] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:408` `dependency should not be added to current lockfile if it was not built successfully during headless install` covers failed build during frozen/headless install.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:445` `selectively ignore scripts in some dependencies by allowBuilds (not others)` covers frozen reinstall with selective build policy.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:466` `selectively allow scripts in some dependencies by allowBuilds` covers frozen reinstall and ignored script reporting.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:504` `selectively allow scripts in some dependencies by allowBuilds using exact versions` covers exact-version allow list.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:552` `lifecycle scripts run after linking root dependencies` verifies builds can require root dependencies during frozen install.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:724` `build dependencies that were not previously built after allowBuilds changes` covers rebuilding newly allowed dependencies with frozen install.
- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:1902` `link the bin file of a workspace project that is created by a lifecycle script` covers workspace build-created bin behavior and frozen reinstall.

Supporting tests:

- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:362` `run pre/postinstall scripts` verifies headless build execution and `pendingBuilds` when scripts are ignored.
- [ ] `TypeScript repo: pnpm/test/install/lifecycleScripts.ts:245` `the list of ignored builds is preserved after a repeat install` covers CLI-level `.modules.yaml.ignoredBuilds` persistence.
- [ ] `TypeScript repo: pnpm/test/install/lifecycleScripts.ts:303` `strictDepBuilds fails for packages with cached side-effects (#11035)` ensures cached side effects do not bypass build approval.
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:26` `run pre/postinstall scripts`
- [ ] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:60` `return the list of packages that should be build`
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:121` `run install scripts`
- [ ] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:175` `installation fails if lifecycle script fails`
- [x] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:303` `run lifecycle scripts of dependent packages after running scripts of their deps`

Rust port notes:

- Port the frozen/headless tests before broad lifecycle coverage.
- Current-lockfile behavior on failed build should be paired with the current-lockfile TODO.

## Support Side-Effects Cache

Primary side-effects tests:

- [ ] `TypeScript repo: installing/deps-installer/test/install/sideEffects.ts:79` `using side effects cache` covers side-effects read/write. The test intentionally removes `pnpm-lock.yaml` to avoid headless, so it is cache-first rather than frozen-first.
- [ ] `TypeScript repo: installing/deps-installer/test/install/sideEffects.ts:166` `uploading errors do not interrupt installation` verifies cache upload errors do not fail install.
- [ ] `TypeScript repo: installing/deps-installer/test/install/sideEffects.ts:189` `a postinstall script does not modify the original sources added to the store` verifies side effects stay separate from original CAFS files.
- [ ] `TypeScript repo: installing/deps-installer/test/install/sideEffects.ts:225` `a corrupted side-effects cache is ignored` verifies fallback when cache contents are invalid.

Frozen/headless cross-coverage:

- [ ] `TypeScript repo: installing/deps-installer/test/install/sideEffects.ts:50` `caching side effects of native package when hoisting is used` is skipped upstream but relevant to hoisting plus side-effects cache.
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:24` `patch package with exact version`
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:120` `patch package with version range`
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:297` `patch package when scripts are ignored`
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:386` `patch package when the package is not in allowBuilds list`
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:706` `using side effects cache with nodeLinker=%s` covers headless side-effects behavior for isolated and hoisted linkers.
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:761` `using side effects cache and hoistPattern=*` is skipped upstream but documents intended headless plus hoisting coverage.
- [ ] `TypeScript repo: pnpm/test/install/lifecycleScripts.ts:303` `strictDepBuilds fails for packages with cached side-effects (#11035)` verifies build approval semantics even when side effects are cached.

Rust port notes:

- Do not block side-effects cache porting on patches; port the standalone `sideEffects.ts` tests first.
- The patch tests become important once patched builds and side-effects cache both exist.

## Support Workspaces

Primary frozen/headless tests:

- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:438` `dependencies of other importers are not pruned when (headless) installing for a subset of importers`
- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:208` `install only the dependencies of the specified importer. The current lockfile has importers that do not exist anymore` covers stale importer entries in the current lockfile.
- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:730` `current lockfile contains only installed dependencies when adding a new importer to workspace with shared lockfile` asserts filtered current lockfile contents.
- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:540` `headless install is used when package linked to another package in the workspace`
- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:598` `headless install is used with an up-to-date lockfile when package references another package via workspace: protocol`
- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:656` `headless install is used when packages are not linked from the workspace (unless workspace ranges are used)`
- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:865` `partial installation in a monorepo does not remove dependencies of other workspace projects when lockfile is frozen`
- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:1427` `resolve a subdependency from the workspace` includes frozen reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:1563` `resolve a subdependency from the workspace, when it uses the workspace protocol` includes frozen reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:1768` `symlink local package from the location described in its publishConfig.directory when linkDirectory is true` includes frozen reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:1902` `link the bin file of a workspace project that is created by a lifecycle script` includes frozen reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:341` `hoist-pattern: hoist all dependencies to the virtual store node_modules` covers workspace hoisting and frozen reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:813` `hoistWorkspacePackages should hoist all workspace projects` covers workspace package hoisting and frozen reinstall.

Headless restorer tests:

- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:789` `installing in a workspace`
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:873` `installing in a workspace with node-linker=hoisted`
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:897` `installing a package deeply installs all required dependencies`

CLI-level frozen workspace tests:

- [ ] `TypeScript repo: pnpm/test/monorepo/index.ts:734` `recursive install with shared-workspace-lockfile builds workspace projects in correct order` includes recursive frozen reinstall.
- [ ] `TypeScript repo: pnpm/test/monorepo/index.ts:1281` `dependencies of workspace projects are built during headless installation` runs CLI `install --frozen-lockfile` after lockfile-only generation.
- [ ] `TypeScript repo: pnpm/test/monorepo/index.ts:1317` `linking the package's bin to another workspace package in a monorepo` deletes workspace `node_modules` and runs frozen reinstall.
- [ ] `TypeScript repo: pnpm/test/monorepo/index.ts:1467` `custom virtual store directory in a workspace with not shared lockfile` verifies workspace-local custom virtual store on frozen reinstall.
- [ ] `TypeScript repo: pnpm/test/monorepo/index.ts:1514` `custom virtual store directory in a workspace with shared lockfile` verifies root custom virtual store on frozen reinstall.

Rust port notes:

- Start with single root workspace lockfile and direct workspace links.
- Add subset/partial install tests only after Rust has project selection semantics.

## Support `nodeLinker=hoisted`

Primary tests:

- [ ] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:16` `installing with hoisted node-linker`
- [ ] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:45` `installing with hoisted node-linker and no lockfile`
- [ ] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:61` `overwriting (is-positive@3.0.0 with is-positive@latest)`
- [ ] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:83` `overwriting existing files in node_modules`
- [ ] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:97` `preserve subdeps on update`
- [ ] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:119` `adding a new dependency to one of the workspace projects`
- [ ] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:172` `installing the same package with alias and no alias`
- [ ] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:187` `run pre/postinstall scripts. bin files should be linked in a hoisted node_modules`
- [ ] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:210` `running install scripts in a workspace that has no root project`
- [ ] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:229` `hoistingLimits should prevent packages to be hoisted`
- [ ] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:247` `externalDependencies should prevent package from being hoisted to the root`
- [ ] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:264` `linking bins of local projects when node-linker is set to hoisted`
- [ ] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:314` `peerDependencies should be installed when autoInstallPeers is set to true and nodeLinker is set to hoisted`
- [ ] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:329` `installing with hoisted node-linker a package that is a peer dependency of itself`
- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:87` `install only the dependencies of the specified importer, when node-linker is hoisted` is workspace subset coverage for hoisted linker.

Frozen/headless cross-coverage:

- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:594` `install optional dependency for the supported architecture set by the user (nodeLinker=%s)` includes hoisted frozen install.
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:24` `patch package with exact version` includes frozen hoisted reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:120` `patch package with version range` includes frozen hoisted reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:297` `patch package when scripts are ignored` includes frozen hoisted reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:386` `patch package when the package is not in allowBuilds list` includes frozen hoisted reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:579` `run pre/postinstall scripts in a workspace that uses node-linker=hoisted`
- [ ] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:686` `run pre/postinstall scripts in a project that uses node-linker=hoisted. Should not fail on repeat install`
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:859` `installing with node-linker=hoisted`
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:873` `installing in a workspace with node-linker=hoisted`

Rust port notes:

- Treat hoisted linker as its own milestone. Its tests overlap with optional deps, patches, lifecycle scripts, bins, and workspaces.

## Support The Global Virtual Store Dir

Primary tests:

- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:21` `using a global virtual store` includes reinstall with `frozenLockfile: true`.
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:63` `reinstall from warm global virtual store after deleting node_modules` deletes `node_modules`, keeps GVS warm, and reinstalls frozen.
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:107` `modules are correctly updated when using a global virtual store`
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:132` `GVS hashes are engine-agnostic for packages not in allowBuilds`
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:172` `GVS hashes are stable when allowBuilds targets an unrelated package`
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:205` `GVS re-links when allowBuilds changes`
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:250` `GVS successful build creates package directory with build artifacts`
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:290` `GVS: approve-builds scenario — install with no builds, then reinstall with allowBuilds`
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:338` `GVS build failure cleans up broken package directory`
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:367` `GVS rebuilds successfully after simulated build failure cleanup`
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:411` `GVS .pnpm-needs-build marker triggers re-import on next install`
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:461` `injected local packages work with global virtual store`
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:539` `virtualStoreOnly populates standard virtual store without importer symlinks` is the standard-store counterpart for virtual-store-only behavior.
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:559` `virtualStoreOnly with enableModulesDir=false throws config error (standard virtual store)` is the negative counterpart to GVS behavior.
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:571` `virtualStoreOnly with enableModulesDir=false works when GVS is enabled`
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:605` `virtualStoreOnly with GVS populates global virtual store without importer links`
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:635` `virtualStoreOnly with frozenLockfile populates virtual store without importer symlinks`
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:677` `virtualStoreOnly with frozenLockfile populates standard virtual store without importer symlinks`
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:708` `virtualStoreOnly suppresses hoisting even with explicit hoistPattern`

CLI-level tests:

- [ ] `TypeScript repo: pnpm/test/install/globalVirtualStore.ts:11` `using a global virtual store`
- [ ] `TypeScript repo: pnpm/test/install/globalVirtualStore.ts:34` `approve-builds updates GVS symlinks and runs builds at correct hash directory`
- [ ] `TypeScript repo: pnpm/test/install/globalVirtualStore.ts:80` `warm GVS reinstall skips internal linking`

Rust port notes:

- Separate GVS path layout from build/allowBuilds behavior.
- The first frozen target should be warm GVS reinstall after deleting `node_modules`.

## Link Dependency Binaries

Primary frozen/headless tests:

- [ ] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:331` `lifecycle scripts run before linking bins` verifies generated bins after frozen reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:372` `bins are linked even if lifecycle scripts are ignored` verifies direct and nested bins after frozen reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:567` `the hoisted packages should not override the bin files of the direct dependencies` verifies public hoist bin precedence after frozen reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:1902` `link the bin file of a workspace project that is created by a lifecycle script` verifies workspace bin link after frozen reinstall.

Supporting tests:

- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:54` `installing a simple project` verifies `.bin/rimraf` in headless install.
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:569` `installing with hoistPattern=*` verifies private hoisted `.bin/hello-world-js-bin`.
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:628` `installing with publicHoistPattern=*` verifies public `.bin/hello-world-js-bin`.
- [ ] `TypeScript repo: installing/deps-installer/test/install/misc.ts:1130` `installing with no symlinks with PnP` verifies `.bin` exists with no symlink layout.
- [ ] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:187` `run pre/postinstall scripts. bin files should be linked in a hoisted node_modules`
- [ ] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:264` `linking bins of local projects when node-linker is set to hoisted`

Rust port notes:

- Port direct dependency bins first.
- Then add hoisted-bin precedence and lifecycle-created bins.

## Existing `node_modules` With Existing Packages

Primary frozen/headless tests:

- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:237` `installing non-prod deps then all deps` verifies headless repeat install adds missing dependency groups and updates install state.
- [ ] `TypeScript repo: installing/deps-installer/test/install/misc.ts:844` `reinstalls missing packages to node_modules during headless install` starts with existing install, removes package links/store locations, and verifies install repairs `node_modules`.
- [ ] `TypeScript repo: installing/deps-installer/test/lockfile.ts:547` `repeat install with no inner lockfile should not rewrite packages in node_modules` verifies reinstall keeps existing packages usable when `node_modules/.pnpm/lock.yaml` is absent.
- [ ] `TypeScript repo: installing/deps-installer/test/install/hoist.ts:24` `should hoist dependencies` verifies repeat installs preserve existing hoisted packages under frozen/headless install.
- [ ] `TypeScript repo: installing/deps-installer/test/packageImportMethods.ts:31` `packages are updated in node_modules, when packageImportMethod is set to copy and modules manifest and current lockfile are incorrect` corrupts both install-state files and verifies `node_modules` is repaired.

Supporting tests:

- [ ] `TypeScript repo: installing/deps-installer/test/install/misc.ts:784` `rewrites node_modules created by npm` is relevant to pre-existing `node_modules`, but not frozen/headless.
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:432` `available packages are used when node_modules is not clean` is headless-restorer behavior around dirty `node_modules`.
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:469` `available packages are relinked during forced install` covers force-path relinking with existing packages.
- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:63` `reinstall from warm global virtual store after deleting node_modules` repairs project links from a warm GVS.
- [ ] `TypeScript repo: pnpm/test/install/globalVirtualStore.ts:80` `warm GVS reinstall skips internal linking` is CLI-level existing-`node_modules`/warm-GVS coverage.

Rust port notes:

- First test target: do not assume clean `node_modules`.
- Preserve user/unrelated files and repair missing package links without rewriting everything unnecessarily.

## Write And Update Current Lockfile At `node_modules/.pnpm/lock.yaml`

Primary tests:

- [ ] `TypeScript repo: installing/deps-installer/test/install/globalVirtualStore.ts:21` `using a global virtual store` verifies `node_modules/.pnpm/lock.yaml` exists after install and frozen reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/packageExtensions.ts:16` `manifests are extended with fields specified by packageExtensions` verifies wanted lockfile checksum matches current lockfile, including after frozen install.
- [ ] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:408` `dependency should not be added to current lockfile if it was not built successfully during headless install` verifies failed build does not update current lockfile.
- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:74` `skip optional dependency that does not support the current OS` verifies current lockfile package set matches wanted lockfile while skipped packages are tracked.
- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:208` `install only the dependencies of the specified importer. The current lockfile has importers that do not exist anymore` covers stale current-lockfile importers.
- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:730` `current lockfile contains only installed dependencies when adding a new importer to workspace with shared lockfile` verifies filtered current lockfile content.
- [ ] `TypeScript repo: installing/deps-installer/test/packageImportMethods.ts:31` `packages are updated in node_modules, when packageImportMethod is set to copy and modules manifest and current lockfile are incorrect` covers incorrect current lockfile repair.
- [ ] `TypeScript repo: installing/deps-installer/test/lockfile.ts:368` `subdeps are updated on repeat install if outer pnpm-lock.yaml does not match the inner one` tests wanted/current lockfile divergence.
- [ ] `TypeScript repo: installing/deps-installer/test/lockfile.ts:547` `repeat install with no inner lockfile should not rewrite packages in node_modules` covers missing current lockfile on repeat install.
- [ ] `TypeScript repo: installing/deps-installer/test/lockfile.ts:1007` `use current pnpm-lock.yaml as initial wanted one, when wanted was removed` covers recovering from current lockfile when wanted lockfile is gone.
- [ ] `TypeScript repo: installing/deps-installer/test/lockfile.ts:1351` `a broken private lockfile is ignored` covers malformed `node_modules/.pnpm/lock.yaml`.
- [ ] `TypeScript repo: installing/deps-installer/test/lockfile.ts:1324` `a lockfile with duplicate keys causes an exception, when frozenLockfile is true` covers frozen lockfile parse/validation failure.

Supporting tests:

- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:54` `installing a simple project` verifies current lockfile exists after headless install.
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:165` `installing with package manifest ignored` verifies filtered current lockfile package contents.
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:189` `installing only prod package with package manifest ignored` verifies filtered current lockfile package contents.
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:213` `installing only dev package with package manifest ignored` verifies filtered current lockfile package contents.
- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:789` `installing in a workspace` verifies current lockfile is filtered after subset workspace headless install.

Rust port notes:

- Port the simple write first, then filtered lockfiles, then negative failed-build behavior.

## Progress Reporting Matching pnpm

Reporter unit tests:

- [ ] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:25` `prints progress beginning`
- [ ] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:50` `prints progress without added packages stats`
- [ ] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:78` `prints all progress stats`
- [ ] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:119` `prints progress beginning of node_modules from not cwd`
- [ ] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:144` `prints progress beginning of node_modules from not cwd, when progress prefix is hidden`
- [ ] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:172` `prints progress beginning when appendOnly is true`
- [ ] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:200` `prints progress beginning during recursive install`
- [ ] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:228` `prints progress on first download`
- [ ] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:262` `moves fixed line to the end`
- [ ] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:307` `prints "Already up to date"`
- [ ] `TypeScript repo: cli/default-reporter/test/reportingProgress.ts:324` `prints progress of big files download`

Install reporter coverage:

- [ ] `TypeScript repo: installing/deps-restorer/test/index.ts:54` `installing a simple project` asserts headless reporter events: stats, stage, package-manifest, and resolved logs.

Rust port notes:

- Port event-shape tests separately from terminal rendering.
- For terminal rendering, snapshot exact text only after event parity exists.

## Support Proper Auth

Install tests:

- [ ] `TypeScript repo: installing/deps-installer/test/install/auth.ts:14` `a package that need authentication`
- [ ] `TypeScript repo: installing/deps-installer/test/install/auth.ts:52` `installing a package that need authentication, using password`
- [ ] `TypeScript repo: installing/deps-installer/test/install/auth.ts:73` `a package that need authentication, legacy way`
- [ ] `TypeScript repo: installing/deps-installer/test/install/auth.ts:94` `a scoped package that need authentication specific to scope`
- [ ] `TypeScript repo: installing/deps-installer/test/install/auth.ts:142` `a scoped package that need legacy authentication specific to scope`
- [ ] `TypeScript repo: installing/deps-installer/test/install/auth.ts:190` `a package that need authentication reuses authorization tokens for tarball fetching`
- [ ] `TypeScript repo: installing/deps-installer/test/install/auth.ts:216` `a package that need authentication reuses authorization tokens for tarball fetching when meta info is cached`

Auth header tests:

- [x] `TypeScript repo: network/auth-header/test/getAuthHeadersFromConfig.test.ts:32` `should convert auth token to Bearer header`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeadersFromConfig.test.ts:42` `should convert basicAuth to Basic header`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeadersFromConfig.test.ts:50` `should handle default registry auth (empty key)`
- [ ] `TypeScript repo: network/auth-header/test/getAuthHeadersFromConfig.test.ts:58` `should execute tokenHelper`
- [ ] `TypeScript repo: network/auth-header/test/getAuthHeadersFromConfig.test.ts:66` `should prepend Bearer to raw token from tokenHelper`
- [ ] `TypeScript repo: network/auth-header/test/getAuthHeadersFromConfig.test.ts:74` `should throw an error if the token helper fails`
- [ ] `TypeScript repo: network/auth-header/test/getAuthHeadersFromConfig.test.ts:79` `should throw an error if the token helper returns an empty token`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeaderByURI.ts:11` `getAuthHeaderByURI()`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeaderByURI.ts:22` `getAuthHeaderByURI() basic auth without settings`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeaderByURI.ts:30` `getAuthHeaderByURI() basic auth with settings`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeaderByURI.ts:41` `getAuthHeaderByURI() https port 443 checks`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeaderByURI.ts:49` `getAuthHeaderByURI() when default ports are specified`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeaderByURI.ts:61` `getAuthHeaderByURI() when the registry has pathnames`
- [x] `TypeScript repo: network/auth-header/test/getAuthHeaderByURI.ts:72` `getAuthHeaderByURI() with default registry auth`

Auth config parsing and precedence tests:

- [ ] `TypeScript repo: config/reader/test/index.ts:481` `auth tokens from pnpm auth file override ~/.npmrc`
- [ ] `TypeScript repo: config/reader/test/index.ts:523` `workspace .npmrc overrides pnpm auth file`
- [x] `TypeScript repo: config/reader/test/parseCreds.test.ts:15` `authToken`
- [x] `TypeScript repo: config/reader/test/parseCreds.test.ts:23` `authPairBase64`
- [x] `TypeScript repo: config/reader/test/parseCreds.test.ts:49` `authUsername and authPassword`
- [ ] `TypeScript repo: config/reader/test/parseCreds.test.ts:69` `tokenHelper`

Fetcher tests:

- [ ] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:349` `throw error when accessing private package w/o authorization`
- [ ] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:409` `accessing private packages`
- [ ] `TypeScript repo: network/fetch/test/fetchFromRegistry.test.ts:62` `authorization headers are removed before redirection if the target is on a different host`
- [ ] `TypeScript repo: network/fetch/test/fetchFromRegistry.test.ts:90` `authorization headers are not removed before redirection if the target is on the same host`
- [ ] `TypeScript repo: resolving/npm-resolver/test/index.ts:934` `error is thrown when package needs authorization`

Rust port notes:

- Frozen install still fetches tarballs when the store is cold, so auth applies even without resolution.
- Header matching and token helper behavior should be ported below install-level tests.

## Support pnpm Proxy Settings

Proxy dispatcher tests:

- [ ] `TypeScript repo: network/fetch/test/dispatcher.test.ts:62` `returns ProxyAgent for httpProxy with http target`
- [ ] `TypeScript repo: network/fetch/test/dispatcher.test.ts:69` `returns ProxyAgent for httpsProxy with https target`
- [ ] `TypeScript repo: network/fetch/test/dispatcher.test.ts:76` `adds protocol prefix when proxy URL has none`
- [ ] `TypeScript repo: network/fetch/test/dispatcher.test.ts:84` `throws PnpmError for invalid proxy URL`
- [ ] `TypeScript repo: network/fetch/test/dispatcher.test.ts:92` `proxy with authentication credentials`
- [ ] `TypeScript repo: network/fetch/test/dispatcher.test.ts:101` `returns Agent (not ProxyAgent) for socks5 proxy`
- [ ] `TypeScript repo: network/fetch/test/dispatcher.test.ts:111` `returns Agent for socks4 proxy`
- [ ] `TypeScript repo: network/fetch/test/dispatcher.test.ts:119` `returns Agent for socks proxy with https target`
- [ ] `TypeScript repo: network/fetch/test/dispatcher.test.ts:127` `SOCKS proxy dispatchers are cached`
- [ ] `TypeScript repo: network/fetch/test/dispatcher.test.ts:134` `SOCKS proxy can connect through a real SOCKS5 server`
- [ ] `TypeScript repo: network/fetch/test/dispatcher.test.ts:192` `bypasses proxy when noProxy matches hostname`
- [ ] `TypeScript repo: network/fetch/test/dispatcher.test.ts:202` `bypasses proxy when noProxy matches domain suffix`
- [ ] `TypeScript repo: network/fetch/test/dispatcher.test.ts:211` `does not bypass proxy when noProxy does not match`
- [ ] `TypeScript repo: network/fetch/test/dispatcher.test.ts:219` `bypasses proxy when noProxy is true`
- [ ] `TypeScript repo: network/fetch/test/dispatcher.test.ts:228` `handles comma-separated noProxy list`

Config tests:

- [ ] `TypeScript repo: config/reader/test/index.ts:978` `getConfig() converts noproxy to noProxy`
- [ ] `TypeScript repo: config/reader/test/index.ts:1514` `reads proxy settings from global config.yaml`
- [ ] `TypeScript repo: config/reader/test/index.ts:1540` `proxy settings from global config.yaml override .npmrc`
- [ ] `TypeScript repo: config/reader/test/index.ts:1567` `CLI flags override proxy settings from global config.yaml`
- [ ] `TypeScript repo: config/reader/test/index.ts:1592` `proxy settings are still read from .npmrc`
- [ ] `TypeScript repo: config/commands/test/configSet.test.ts:875` `config set --global https-proxy writes to config.yaml, not auth.ini`
- [ ] `TypeScript repo: config/commands/test/configSet.test.ts:902` `config set --global httpProxy writes to config.yaml`
- [ ] `TypeScript repo: config/commands/test/configSet.test.ts:928` `config set --global no-proxy writes to config.yaml`

Rust port notes:

- No direct frozen/headless install proxy test was found. Use these as the parity map for config and network plumbing.
- Add an install-level cold-store frozen test after the Rust fetcher supports proxy injection.

## Installation Of Runtimes

Node runtime tests:

- [ ] `TypeScript repo: installing/deps-installer/test/install/nodeRuntime.ts:209` `installing Node.js runtime` includes frozen/offline reinstall after deleting `node_modules`.
- [ ] `TypeScript repo: installing/deps-installer/test/install/nodeRuntime.ts:332` `installing node.js runtime fails if offline mode is used and node.js not found locally`
- [ ] `TypeScript repo: installing/deps-installer/test/install/nodeRuntime.ts:339` `installing Node.js runtime from RC channel`
- [ ] `TypeScript repo: installing/deps-installer/test/install/nodeRuntime.ts:346` `installing Node.js runtime fails if integrity check fails` verifies frozen integrity failure.
- [ ] `TypeScript repo: installing/deps-installer/test/install/nodeRuntime.ts:400` `installing Node.js runtime for the given supported architecture` includes frozen reinstall for target architecture.
- [ ] `TypeScript repo: installing/deps-installer/test/install/nodeRuntime.ts:422` `installing Node.js runtime, when it is set via the engines field of a dependency`

Deno runtime tests:

- [ ] `TypeScript repo: installing/deps-installer/test/install/denoRuntime.ts:111` `installing Deno runtime` includes frozen/offline reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/denoRuntime.ts:217` `installing Deno runtime fails if offline mode is used and Deno not found locally`
- [ ] `TypeScript repo: installing/deps-installer/test/install/denoRuntime.ts:224` `installing Deno runtime fails if integrity check fails`

Bun runtime tests:

- [ ] `TypeScript repo: installing/deps-installer/test/install/bunRuntime.ts:128` `installing Bun runtime` includes frozen/offline reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/bunRuntime.ts:215` `installing Bun runtime fails if offline mode is used and Bun not found locally`
- [ ] `TypeScript repo: installing/deps-installer/test/install/bunRuntime.ts:222` `installing Bun runtime fails if integrity check fails`

Command-level tests:

- [ ] `TypeScript repo: installing/commands/test/install.ts:124` `install Node.js when devEngines runtime is set with onFail=download`
- [ ] `TypeScript repo: installing/commands/test/install.ts:160` `do not install Node.js when devEngines runtime is not set to onFail=download`
- [ ] `TypeScript repo: pnpm/test/install/runtimeOnFail.ts:8` `runtimeOnFail=download causes Node.js to be downloaded even when the manifest does not set onFail`
- [ ] `TypeScript repo: pnpm/test/install/runtimeOnFail.ts:31` `runtimeOnFail=ignore prevents Node.js download even when manifest sets onFail=download`

Runtime manifest/config conversion tests:

- [ ] `TypeScript repo: config/reader/test/index.ts:85` `nodeVersion from config takes priority over devEngines.runtime`
- [ ] `TypeScript repo: config/reader/test/index.ts:109` `runtimeOnFail=download overrides devEngines.runtime.onFail and adds node to devDependencies`
- [ ] `TypeScript repo: config/reader/test/index.ts:138` `runtimeOnFail=ignore overrides an existing onFail=download and removes node from devDependencies`
- [ ] `TypeScript repo: workspace/project-manifest-reader/test/index.ts:37` `readProjectManifest() converts devEngines runtime to devDependencies`
- [ ] `TypeScript repo: workspace/project-manifest-reader/test/index.ts:68` `readProjectManifest() converts engines runtime to dependencies`

Rust port notes:

- Treat Node, Deno, and Bun as separate subfeatures.
- Frozen/offline reinstall is the most relevant Stage 1 assertion.

## Installation Of Git-Hosted Packages

Install tests:

- [ ] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:31` `from a github repo`
- [ ] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:48` `from a github repo through URL`
- [ ] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:61` `from a github repo with different name via named installation`
- [ ] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:105` `from a github repo with different name`
- [ ] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:150` `a subdependency is from a github repo with different name`
- [ ] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:174` `from a git repo`
- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:206` `from a github repo that has no package.json file` — covered at fetcher level by `crates/git-fetcher/src/fetcher/tests.rs::fetcher_handles_repo_without_package_json`. Full install-level test deferred until a non-resolver lockfile fixture lands.
- [ ] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:276` `re-adding a git repo with a different tag`
- [ ] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:323` `should not update when adding unrelated dependency`
- [ ] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:354` `git-hosted repository is not added to the store if it fails to be built`
- [x] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:366` `from subdirectories of a git repo` — covered at fetcher level by `fetcher_packs_subfolder_when_path_set`. Full install-level test deferred (Stage 2 resolver dependency).
- [ ] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:389` `no hash character for github subdirectory install`
- [ ] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:311` `run prepare script for git-hosted dependencies`

Fetcher/resolver/store tests:

- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:50` `fetch` — `crates/git-fetcher/src/fetcher/tests.rs::fetcher_imports_package_into_cas`.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:69` `fetch a package from Git sub folder` — `fetcher_packs_subfolder_when_path_set`.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:87` `prevent directory traversal attack when using Git sub folder` — `prepare_package::tests::safe_join_path_rejects_escapes` + `cas_io::tests::materialize_into_rejects_traversal`.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:108` `prevent directory traversal attack when using Git sub folder #2` — same coverage as above (`join_checked` rejects every non-`Normal` component variant).
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:129` `fetch a package from Git that has a prepare script` — `fetcher::tests::fetcher_runs_prepare_script_when_allowed`. The test calls `node`/`npm` directly; under-provisioned hosts fail loudly via the existing `.unwrap()` calls (see the "No 'tolerant' tests for missing tools" rule in `pacquet/AGENTS.md`).
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:150` `fetch a package without a package.json` — `fetcher_handles_repo_without_package_json`.
- [ ] `TypeScript repo: fetching/git-fetcher/test/index.ts:169` `fetch a big repository` — perf benchmark, not a correctness test; skip from the porting plan.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:183` `still able to shallow fetch for allowed hosts` — `fetcher::tests::fetcher_uses_shallow_fetch_for_allowed_hosts` (Unix only). A `/bin/sh` shim at `<tempdir>/shim/git` logs every invocation and fakes `rev-parse HEAD`; `PATH` is prepended for the test body via `unsafe { std::env::set_var }` (safe under `cargo nextest`'s one-process-per-test isolation) and the log is parsed to assert the `init` / `remote add origin <url>` / `fetch --depth 1 origin <commit>` sequence. The mirror `fetcher_clones_when_host_not_in_shallow_list` pins the non-shallow branch so the gate's polarity can't drift.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:212` `fail when preparing a git-hosted package` — `fetcher::tests::fetcher_surfaces_prepare_failure`. `node -e "process.exit(1)"` as the prepare script; expects `GitFetcherError::Prepare(PreparePackageError::LifecycleFailed)` carrying `ERR_PNPM_PREPARE_PACKAGE`.
- [ ] `TypeScript repo: fetching/git-fetcher/test/index.ts:230` `fail when preparing a git-hosted package with a partial commit` — Stage 2 (resolver concern).
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:247` `do not build the package when scripts are ignored` — `fetcher_skips_build_when_ignore_scripts`.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:263` `block git package with prepare script` — `fetcher_blocks_build_when_not_allowed`.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:280` `allow git package with prepare script` — `fetcher::tests::fetcher_runs_prepare_when_allow_build_returns_true`. Mirror of the existing block-test (`index.ts:263`) with a per-(name, version) `allow_build` closure returning true; asserts the prepare script's marker file lands in `cas_paths`.
- [x] `TypeScript repo: fetching/git-fetcher/test/index.ts:304` `fetch only the included files` — `tarball_fetcher::tests::filters_files_outside_files_field` (same packlist code path).
- [ ] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:455` `fetch a big repository` — perf benchmark, not a correctness test; skip.
- [ ] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:472` `fail when preparing a git-hosted package` — needs a real failing prepare script. Deferred.
- [x] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:490` `take only the files included in the package, when fetching a git-hosted package` — `tarball_fetcher::tests::filters_files_outside_files_field`.
- [ ] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:534` `do not build the package when scripts are ignored` — git-fetcher equivalent covered (`fetcher_skips_build_when_ignore_scripts`); a tarball-side mirror is straightforward but not yet written.
- [x] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:580` `use the subfolder when path is present` — `tarball_fetcher::tests::path_field_packs_only_subdirectory`.
- [x] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:610` `prevent directory traversal attack when path is present` — `tarball_path_traversal_attack_is_rejected`.
- [x] `TypeScript repo: fetching/tarball-fetcher/test/fetch.ts:637` `fail when path is not exists` — `tarball_path_to_missing_subdir_is_rejected`.
- [ ] `TypeScript repo: resolving/git-resolver/test/index.ts:188` `resolveFromGit() with sub folder`
- [ ] `TypeScript repo: resolving/git-resolver/test/index.ts:211` `resolveFromGit() with both sub folder and branch`
- [ ] `TypeScript repo: resolving/git-resolver/test/index.ts:482` `resolve a private repository using the HTTPS protocol without auth token`
- [ ] `TypeScript repo: resolving/git-resolver/test/index.ts:526` `resolve a private repository using the HTTPS protocol and an auth token`
- [x] `TypeScript repo: installing/package-requester/test/index.ts:884` `fetch a git package without a package.json` — covered alongside `fetching/git-fetcher/test/index.ts:150` via `fetcher_handles_repo_without_package_json`.
- [ ] `TypeScript repo: installing/deps-installer/test/install/peerDependencies.ts:30` `don't fail when peer dependency is fetched from GitHub`
- [ ] `TypeScript repo: installing/deps-installer/test/lockfile.ts:600` `updating package that has a github-hosted dependency`
- [x] `TypeScript repo: store/pkg-finder/test/readPackageFileMap.test.ts:67` `should resolve git-hosted tarball packages (no type, has tarball)` — write side covered by `tarball_fetcher::tests::writes_index_row_when_writer_provided`; read side reuses the existing tarball-warm prefetch (no git-specific code path).
- [x] `TypeScript repo: store/pkg-finder/test/readPackageFileMap.test.ts:84` `should resolve git dependencies with type "git" and return readable file paths` — same coverage: the write side produces a `gitHostedStoreIndexKey` row at `pkg_id\tbuilt` (see `create_virtual_store::tests::snapshot_cache_key_for_git_resolution_uses_git_hosted_key` for the read-side key shape pin).

Skipped upstream tests to track:

- [ ] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:186` `from a non-github git repo`
- [ ] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:232` `from a github repo that needs to be built. isolated node linker is used`
- [ ] `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts:252` `from a github repo that needs to be built. hoisted node linker is  used`

Rust port notes:

- Frozen install should not resolve git specs, but it must materialize git-hosted package entries from the lockfile.
- Port store/fetcher handling before resolver tests if Stage 1 stays strictly lockfile-driven.
