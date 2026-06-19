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

## Workspace Project Filtering (`--filter`)

Ported into the new `pacquet-workspace-projects-filter` and
`pacquet-workspace-projects-graph` crates (the Rust ports of
`@pnpm/workspace.projects-filter` and `@pnpm/workspace.projects-graph`).
The CLI `--filter` / `--filter-prod` flags are parsed into
`Config::filter` / `Config::filter_prod`; narrowing the install to the
selected projects is still a follow-up (the install fan-out is
unfiltered, so the two `known_failures` hoist stubs below stay).

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

Changed-packages (`[<since>]`) selectors — stubbed in
`filter::tests::known_failures` (the selector parses but
`filter_workspace_projects` rejects it with
`FilterError::UnsupportedDiffSelector`; the rejection path is covered by
`filter::tests::diff_selector_is_unsupported`). These need git-diff
project selection (`getChangedProjects`), not yet ported:

- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:348` `select changed packages`. Stubbed (`git_diff_selection_unimplemented`).
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:480` `select changed packages when operating under a git worktree`. Stubbed.
- [x] `TypeScript repo: workspace/projects-filter/test/index.ts:553` `selection should fail when diffing to a branch that does not exist`. Stubbed.

`createProjectsGraph` has no upstream unit tests (it is exercised only
through `filterProjectsFromDir`'s fixtures upstream); pacquet covers it
with `create_projects_graph::tests` (workspace-spec, version/range,
local-path, strict `linkWorkspacePackages`, and `ignoreDevDeps` edge
resolution).

## Support `nodeLinker=hoisted`

Primary tests:

- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:16` `installing with hoisted node-linker`. Ported as `installing_with_hoisted_node_linker` in `crates/cli/tests/hoisted_node_linker.rs` (real dirs at root + version-conflict nesting + `.modules.yaml` linker). The rimraf-then-reinstall re-add tail is the partial-install path (pnpm/pacquet#433) and is omitted.
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:45` `installing with hoisted node-linker and no lockfile`. Ported as `installing_with_hoisted_node_linker_and_no_lockfile` (real dir + no `pnpm-lock.yaml` when `lockfile: false`).
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:61` `overwriting (is-positive@3.0.0 with is-positive@latest)`. Stubbed in `known_failures::overwriting_is_positive_with_latest` — needs `pnpm add` / update manifest mutation (pnpm/pacquet#433).
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:83` `overwriting existing files in node_modules`. Stubbed in `known_failures::overwriting_existing_files_in_node_modules` (#433).
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:97` `preserve subdeps on update`. Stubbed in `known_failures::preserve_subdeps_on_update` (#433).
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:119` `adding a new dependency to one of the workspace projects`. Stubbed in `known_failures::adding_a_new_dependency_to_a_workspace_project` (#433).
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:172` `installing the same package with alias and no alias`. Stubbed in `known_failures::installing_same_package_with_alias_and_no_alias` — needs `pnpm add` of multiple specifiers + a dist-tag bump (#433).
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:187` `run pre/postinstall scripts. bin files should be linked in a hoisted node_modules`. Stubbed in `known_failures::run_pre_and_postinstall_scripts_and_link_bins` — lifecycle scripts + bin linking on the fresh path (#11870).
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:210` `running install scripts in a workspace that has no root project`. Stubbed in `known_failures::running_install_scripts_in_workspace_without_root_project` (#11870).
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:229` `hoistingLimits should prevent packages to be hoisted`. Ported as `hoisting_limits_prevents_hoisting` (`hoistingLimits: dependencies`). Pacquet's `hoistingLimits` config was migrated from the raw locator map to the `none`/`workspaces`/`dependencies` enum to match the pnpm CLI setting, and `real-hoist`'s border semantics were corrected (a name in the limits is a subtree border whose descendants stay nested, matching the `@yarnpkg/nm` hoister).
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:247` `externalDependencies should prevent package from being hoisted to the root`. Ported as `external_dependencies_prevents_hoisting_to_root`.
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:264` `linking bins of local projects when node-linker is set to hoisted`. Stubbed in `known_failures::linking_bins_of_local_projects` (#11870 — bin linking on the fresh path).
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:314` `peerDependencies should be installed when autoInstallPeers is set to true and nodeLinker is set to hoisted`. Ported as `peer_dependencies_installed_with_auto_install_peers`.
- [x] `TypeScript repo: installing/deps-installer/test/hoistedNodeLinker/install.ts:329` `installing with hoisted node-linker a package that is a peer dependency of itself`. Stubbed in `known_failures::package_that_is_peer_dependency_of_itself` — needs `pnpm add --save` + lockfile `peerDependencies` introspection (#433).
- [ ] `TypeScript repo: installing/deps-installer/test/install/multipleImporters.ts:87` `install only the dependencies of the specified importer, when node-linker is hoisted` is workspace subset coverage for hoisted linker.

Frozen/headless cross-coverage:

- [ ] `TypeScript repo: installing/deps-installer/test/install/optionalDependencies.ts:594` `install optional dependency for the supported architecture set by the user (nodeLinker=%s)` includes hoisted frozen install.
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:24` `patch package with exact version` includes frozen hoisted reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:120` `patch package with version range` includes frozen hoisted reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:297` `patch package when scripts are ignored` includes frozen hoisted reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/patch.ts:386` `patch package when the package is not in allowBuilds list` includes frozen hoisted reinstall.
- [ ] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:579` `run pre/postinstall scripts in a workspace that uses node-linker=hoisted`
- [ ] `TypeScript repo: installing/deps-installer/test/install/lifecycleScripts.ts:686` `run pre/postinstall scripts in a project that uses node-linker=hoisted. Should not fail on repeat install`
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:859` `installing with node-linker=hoisted`. Ported as `installing_with_hoisted_node_linker_frozen` in `crates/cli/tests/hoisted_node_linker.rs` — seeds the lockfile with a fresh install, tears down `node_modules`, then replays via `--frozen-lockfile` and asserts the real-dir + version-conflict-nesting layout.
- [x] `TypeScript repo: installing/deps-restorer/test/index.ts:873` `installing in a workspace with node-linker=hoisted`. Ported as `installing_in_a_workspace_with_hoisted_node_linker_frozen` — a frozen workspace replay where the root importer's `ms@2.1.3` wins the top-level slot and a project's conflicting `ms@2.0.0` nests under the project (the root-deps-rank-first preference landed in `real-hoist`).

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
- [x] `TypeScript repo: installing/deps-installer/test/install/packageExtensions.ts:16` `manifests are extended with fields specified by packageExtensions` — split into pacquet's `install::tests::fresh_install_applies_package_extensions_to_dependency_manifest` (verifies the extension lands in the lockfile's `packages` block AND `packageExtensionsChecksum` is written) and `install::tests::frozen_lockfile_errors_when_package_extensions_drift_from_lockfile` (frozen-install drift gate). Current-lockfile round-trip parity is covered by `current_lockfile`'s clone of `package_extensions_checksum`.
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

The runtime lockfile *format* (importer `version: runtime:<ver>`, the
`packages[node@runtime:<ver>].version: <ver>` field, and the
`variants[].resolution.bin: { node: … }` map asserted in
`nodeRuntime.ts:236-269`) is covered at pacquet's adapter/resolver layer by
`dependencies_graph_to_lockfile::tests::runtime_dependency_strips_importer_prefix_and_records_package_version`
and `node_resolver::tests::bin_spec_is_a_named_map`. The full
install-and-reinstall integration tests below are still unported (they
download real runtime artifacts).

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
- [ ] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:542` `does not fail with ERR_PNPM_MISSING_TIME when package@version is excluded and time field is missing` — exclude-then-missing-time interplay isn't pinned yet; an upstream-style test still needs to land in `trust_checks::tests`.
- [ ] `TypeScript repo: resolving/npm-resolver/test/trustChecks.test.ts:564` `does not fail with ERR_PNPM_MISSING_TIME when package name is excluded and time field is missing` — same as above (name-pattern variant).

### Attestation publish-time fetcher

- [x] `TypeScript repo: resolving/npm-resolver/test/fetchAttestationPublishedAt.test.ts:35` `returns an ISO timestamp built from tlogEntries[].integratedTime` — `fetch_attestation_published_at::tests::finds_publish_time_from_single_bundle`.
- [x] `TypeScript repo: resolving/npm-resolver/test/fetchAttestationPublishedAt.test.ts:75` `returns undefined when the registry has no attestations for the package (404)` — `fetch_attestation_published_at::tests::returns_none_on_404`.
- [x] `TypeScript repo: resolving/npm-resolver/test/fetchAttestationPublishedAt.test.ts:86` `returns undefined on 5xx — caller falls back to full metadata` — `fetch_attestation_published_at::tests::returns_none_on_5xx`.
- [x] `TypeScript repo: resolving/npm-resolver/test/fetchAttestationPublishedAt.test.ts:110` `returns undefined when the body is malformed JSON` — `fetch_attestation_published_at::tests::returns_none_on_malformed_body`.
- [x] `TypeScript repo: resolving/npm-resolver/test/fetchAttestationPublishedAt.test.ts:153` `picks the earliest integratedTime across multiple attestations` — `fetch_attestation_published_at::tests::earliest_wins_across_multiple_bundles`.
- [x] `TypeScript repo: resolving/npm-resolver/test/fetchAttestationPublishedAt.test.ts:169` `accepts integratedTime as a number too (defensive against schema drift)` — `fetch_attestation_published_at::tests::accepts_integrated_time_as_number`.
- [x] `TypeScript repo: resolving/npm-resolver/test/fetchAttestationPublishedAt.test.ts:198` `strips a trailing slash on the registry URL` — `fetch_attestation_published_at::tests::trims_trailing_slash_from_registry_root`.

### `createNpmResolutionVerifier`

- [x] `TypeScript repo: resolving/npm-resolver/test/createNpmResolutionVerifier.test.ts:48` `createNpmResolutionVerifier() returns undefined when no policy is active` — `create_npm_resolution_verifier::tests::returns_none_when_no_policy_active` (plus the `returns_none_when_min_age_is_zero` / `returns_none_when_trust_policy_off` siblings that pin the off-by-one cases).
- [x] `TypeScript repo: resolving/npm-resolver/test/createNpmResolutionVerifier.test.ts:52` `createNpmResolutionVerifier() flags a trustedPublisher → provenance downgrade` — `create_npm_resolution_verifier::tests::trust_downgrade_publisher_to_provenance_fails`.
- [x] `TypeScript repo: resolving/npm-resolver/test/createNpmResolutionVerifier.test.ts:99` `createNpmResolutionVerifier() passes a same-evidence-level version` — `create_npm_resolution_verifier::tests::trust_downgrade_pass_when_no_weaker_evidence`.
- [ ] `TypeScript repo: resolving/npm-resolver/test/createNpmResolutionVerifier.test.ts:141` `abbreviated shortcut requires the pinned version to be in metadata` — the abbreviated-modified shortcut is deferred (Phase 4 stubs that layer); rerun when Phase 5+ ports `fetchAbbreviatedMetadataCached`.
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
- [ ] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:143` `does not collapse same (name, version) with different resolutions` — pacquet's collector keys by `(name, version, JSON(resolution))` (see `collect_candidates` in `verify_lockfile_resolutions.rs`), but a regression test pinning that contract is not yet written.
- [ ] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:166` `the verifier sees the resolution shape verbatim` — same gap (the protocol-pass-through is exercised today only via the npm-verifier's own tarball-vs-registry tests).
- [ ] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutions.ts:264` `does not write a cache record when verification rejects` — currently relies on inspection of the cache file being absent; a dedicated test still needs to land in `cache::tests`.

### Cache (`tryLockfileVerificationCache`, `recordVerification`)

- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:46` `miss when the cache file does not exist` — `cache::tests::cold_cache_misses_with_populated_stat`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:69` `stat-only hit when size, mtime, and inode all match` — `cache::tests::stat_shortcut_hits_same_path_same_stat`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:132` `miss when a verifier rejects the cached policy` — `cache::tests::policy_invalidation_misses_even_when_stat_matches`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:205` `hit at a new path when the content matches a cached hash (worktree case)` — `cache::tests::content_hash_lookup_finds_same_lockfile_at_different_path`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:256` `malformed lines are ignored, not propagated` — `cache::tests::malformed_lines_are_tolerated_on_read`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:273` `writes a JSONL record with a merged policy bag` — `cache::tests::record_verification_merges_policies`.
- [x] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:341` `appends without rewriting previous lines` — `cache::tests::append_only_log_records_each_call`.
- [ ] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:55` `miss when the lockfile path is not in the cache` — implicitly covered by `cold_cache_misses_with_populated_stat`, but an upstream-style explicit test is missing.
- [ ] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:80` `stat shortcut bails on size mismatch and falls through to hash lookup` — not yet ported.
- [ ] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:97` `hash-fallback hit when size matches but mtime/inode were reset` — not yet ported.
- [ ] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:116` `miss when content changed even if size happens to match` — not yet ported.
- [ ] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:144` `hit when a verifier accepts the cached policy` — implicit in the round-trip tests; explicit upstream-style test missing.
- [ ] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:176` `hit when every verifier trusts its share of the merged cached policy` — multi-verifier merge happy-path is not yet pinned.
- [ ] `TypeScript repo: installing/deps-installer/test/install/verifyLockfileResolutionsCache.ts:193` `miss when the lockfile no longer exists` — missing-lockfile branch returns `hit: false`, not yet pinned.
- [x] cache compaction past 1.5 MB — `cache::tests::compaction_dedupes_by_path_and_hash`.

### `recordLockfileVerified` wrapper

- [x] `TypeScript repo: installing/deps-installer/test/install/recordLockfileVerified.ts:62` `no-op when cacheDir is undefined` — `record_lockfile_verified` short-circuits on `cache_dir.is_none()`; pinned indirectly via the runner's `second_run_with_cache_skips_fan_out` (which exercises the recorder when caching is on).
- [x] `TypeScript repo: installing/deps-installer/test/install/recordLockfileVerified.ts:72` `no-op when resolutionVerifiers is empty` — same shape as the cache-dir guard.
- [ ] `TypeScript repo: installing/deps-installer/test/install/recordLockfileVerified.ts:103` `records the load-equivalent hash — matches what the next install computes off-disk` — would benefit from a dedicated round-trip test that loads a written lockfile and reads it back; pacquet's `hash_lockfile::tests::key_order_in_yaml_does_not_affect_hash` is close but not the same path.
- [ ] `TypeScript repo: installing/deps-installer/test/install/recordLockfileVerified.ts:141` `respects the caller-supplied lockfilePath` — git-branch-suffixed lockfile case is not yet pinned.

### `minimumReleaseAge` install-side behavior

- [x] `TypeScript repo: installing/deps-installer/test/install/minimumReleaseAge.ts:15` `prevents installation of versions that do not meet the required publish date cutoff` — covered end-to-end by `pacquet-package-manager::install::tests::frozen_lockfile_gate_rejects_under_huge_minimum_release_age` and the CLI integration test `cli::lockfile_verification::install_fails_under_huge_minimum_release_age`.
- [x] `TypeScript repo: installing/deps-installer/test/install/minimumReleaseAge.ts:23` `ignored for packages in the minimumReleaseAgeExclude array` — `create_npm_resolution_verifier::tests::verify_skips_age_check_when_package_excluded`.
- [x] `TypeScript repo: installing/deps-installer/test/install/minimumReleaseAge.ts:128` `throws error when semver range is used in minimumReleaseAgeExclude` — `pacquet-package-manager::install::tests::install_rejects_invalid_minimum_release_age_exclude_pattern`.
- [ ] `TypeScript repo: installing/deps-installer/test/install/minimumReleaseAge.ts:32` `ignored using a pattern` — wildcard exclude (`foo-*`) isn't pinned in pacquet's tests today; the exclude policy supports it.
- [ ] `TypeScript repo: installing/deps-installer/test/install/minimumReleaseAge.ts:41` `ignored for specific exact versions in minimumReleaseAgeExclude` — version-union excludes (`foo@1.0.0 || 1.1.0`) aren't pinned end-to-end yet.
- [ ] `TypeScript repo: installing/deps-installer/test/install/minimumReleaseAge.ts:68` `falls back to immature version when no mature version satisfies the range (non-strict mode)` — the fall-back-on-non-strict path lives in the resolver, which pacquet doesn't have yet; out of scope until the resolver lands.
- [ ] `TypeScript repo: installing/deps-installer/test/install/minimumReleaseAge.ts:86` `strict minimumReleaseAge surfaces every immature pick via handleResolutionPolicyViolations, then aborts` — same gating; resolver-dependent.
- [ ] `TypeScript repo: installing/deps-installer/test/install/minimumReleaseAge.ts:140` `enforced on an existing lockfile entry that does not meet the cutoff` — partially covered by the existing e2e tests (the gate runs from a lockfile); a closer mirror of upstream's fixture-with-timestamps shape isn't ported yet.

### Version-policy parser

- [x] `TypeScript repo: config/version-policy/test/index.ts:8` `createPackageVersionPolicy()` — `pacquet-config::version_policy::tests` exhaustive coverage of the parsing + matcher contract.
- [x] `TypeScript repo: config/version-policy/test/index.ts:57` `createPackageVersionPolicyOrThrow() rewraps parser errors with INVALID_<KEY>` — handled at the install boundary in `pacquet-package-manager::build_resolution_verifiers` (wraps `VersionPolicyError` → `BuildVerifiersError::InvalidMinimumReleaseAgeExclude` / `InvalidTrustPolicyExclude`).

Rust port notes:

- The abbreviated-modified shortcut and the on-disk `local_meta` layer are stubbed in Phase 4 / Phase 5 (no `fetchAbbreviatedMetadataCached` port yet). Upstream tests that depend on those layers stay unchecked until the abbreviated-cache fetcher lands.
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
- [ ] `TypeScript repo: deps/status/test/checkDepsStatus.test.ts:175` `returns upToDate: false when peersSuffixMaxLength has changed` — still bundled under `returns_up_to_date_when_state_carries_unported_pnpm_settings`. Pacquet doesn't yet read `peersSuffixMaxLength` into `Config`; split out once it lands.

### Not yet ported

- [ ] `TypeScript repo: deps/status/test/checkDepsStatus.test.ts:234` `skips the allowBuilds change detection when allowBuilds is in ignoredWorkspaceStateSettings` — `ignoredWorkspaceStateSettings` is a per-call ignore list (`opts.ignoredWorkspaceStateSettings`) that pacquet's `check_optimistic_repeat_install` does not accept yet. Add the field when porting the second consumer of `checkDepsStatus` (`verifyDepsBeforeRun`).
- [ ] `TypeScript repo: deps/status/test/checkDepsStatus.test.ts:270` `returns upToDate: false when a pnpmfile was modified` — pacquet doesn't run pnpmfiles and writes `pnpmfiles: []` unconditionally. Port alongside the pnpmfile pipeline.
- [ ] `TypeScript repo: deps/status/test/checkDepsStatus.test.ts:328` `returns upToDate: false when a patch was modified and manifests were not modified` — needs `patchesOrHooksAreModified` (stats `patches/<name>.patch` files against `lastValidatedTimestamp`). Falls outside the MVP scope of the mtime-only branch.
- [ ] `TypeScript repo: deps/status/test/checkDepsStatus.test.ts:405` `returns upToDate: false when the wanted lockfile has merge conflict markers` and `:438` `returns upToDate: false when a project lockfile has merge conflict markers and sharedWorkspaceLockfile is false` — needs `findConflictedLockfileDir` (scans lockfile bytes for `<<<<<<<` markers). Outside MVP scope.

Each unported entry above gates the optimistic short-circuit on a code path pacquet does not yet have. The fall-through is safe — when the optimistic check returns `Skipped`, the install runs the regular pipeline, which still has its own freshness guards. None of the unported branches can silently mask drift; they only become relevant once pacquet *enables* the feature in question.

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

- [ ] `TypeScript repo: installing/commands/test/saveCatalog.ts` — the `--save-catalog` / `--save-catalog-name` CLI surface is wired and unit-tested via `catalog_mode::tests::save_catalog_name_*`, but the command-level `saveCatalog.ts` flows (e.g. interaction with `--save-dev`, recursive installs) are not yet ported as CLI integration tests.
- [ ] `TypeScript repo: installing/deps-installer/test/catalogs.ts` general integration cases (`:58` `installing with "catalog:" should work`, `:176` `lockfile contains catalog snapshots`, `:849` snapshot-pruning, the multi-project `--filter` cases) — pacquet covers the catalog-snapshot *emission* via unit tests but hasn't ported these install-level / workspace integration flows.
- [ ] `cleanupUnusedCatalogs` (the `removePackagesFromWorkspaceCatalog` half of the writer) is not ported — pacquet's writer only adds/updates catalog entries.
- [ ] Manual-mode `update --latest` of a `catalog:` dependency: pacquet's catalog handling is gated on `catalogMode != manual`, so under the default manual mode such an update still rewrites the manifest to the version (pre-existing pacquet behavior). The strict/prefer paths match pnpm.

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

- [ ] `multi-project: different peer versions produce different instances` — needs the multi-importer `resolve_peers_workspace` harness; general workspace peer-separation, partially covered by `dedupes_when_the_same_package_appears_in_two_subtrees`.
- [ ] `resolve peer dependencies with npm aliases` — npm-alias peer suffixes.
- [ ] `should find peer dependency conflicts when the peer is an optional peer of one of the dependencies`, `should ignore conflicts between missing optional peer dependencies`, `should pick the single wanted peer dependency range`, `should return the intersection of two compatible ranges`, the two prerelease-warning cases — peer-issue reporting edge cases.
- [ ] The `lockedPeerContext` / `resolvedPeerProviderPaths` series (`prefers a compatible locked provider …`, the six `does not replace …` cases, `does not reuse a locked provider outside the current peer range`) — pacquet hasn't ported `lockedPeerContext`/`resolvedPeerProviderPaths`, so these gate on that feature, not on the lockfile-parity peer fixes.
