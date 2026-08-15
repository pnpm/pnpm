## 1103.2.0

### Minor Changes

- Added a new setting `minimumReleaseAgeExcludePrune`. When enabled, `pnpm add`, `pnpm update`, and `pnpm remove` prune the entries of `minimumReleaseAgeExclude` in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves: versions that are gone are dropped (an entry is removed once none of its versions remain), and entries for packages that are no longer in the lockfile are removed too. Name patterns (`@scope/*`) are always kept. The cleanup is skipped when the install's lockfile does not cover the whole workspace (`sharedWorkspaceLockfile: false`), since entries another project still needs would look stale.

  Renamed `cleanupUnusedCatalogs` to `catalogPrune`, so that catalog pruning and release-age exclude pruning use one vocabulary. `cleanupUnusedCatalogs` continues to work; when both are set, `catalogPrune` wins.

### Patch Changes

- `pnpm add` no longer re-resolves the dependency graph when `pnpm-lock.yaml` already holds a version satisfying the request — promoting a transitive dependency to a direct one, or adding to a second workspace package what a first one already depends on, now only saves the dependency in `package.json` and records its importer entry. A satisfying locked version is necessary but not sufficient: the install still falls back to a full resolution for a dist tag, an alias, a `workspace:`/`catalog:`/git/tarball specifier, `--save-peer`, an overridden package, a `catalogMode` other than `manual`, and — under `resolutionMode: time-based` or `lowest-direct`, which resolve a direct dependency to the low end of its range — a range several locked versions satisfy.

- `pnpm add <pkg>@<version>` and `pnpm update <pkg>@<version>` under a non-manual `catalogMode` now move the catalog entry's resolution to the requested version. Previously, when the catalog entry was a range that covered the requested version but resolved to a different one, the request was dropped silently: nothing was installed, nothing was written, and no error was raised.

- A project that wasn't part of an install that moved a catalog entry now follows the entry the next time it is installed. It used to keep the version the entry resolved to before — a version the entry no longer allowed — and no later install corrected it, so one catalog entry ended up resolved to two versions.

- `pnpm add <pkg>@<version>` and `pnpm update <pkg>@<version>` under `catalogMode: strict` no longer fail with `ERR_PNPM_CATALOG_VERSION_MISMATCH` when the catalog entry is a range that the wanted version satisfies. The dependency keeps using the catalog; only a version that really falls outside the catalog's range is rejected [#13715](https://github.com/pnpm/pnpm/issues/13715).

- A changed `catalogs` or `pnpm.overrides` block no longer has to be the only change for `pnpm install` to update the lockfile in place. Editing an override while also removing a dependency, or changing a catalog entry in the same commit as a range bump, is now absorbed in one pass instead of re-resolving the whole dependency graph [#13799](https://github.com/pnpm/pnpm/issues/13799).

  Fixed the lockfile an in-place override update wrote when the overridden package was also a catalog entry: the entry kept the version it had before the override moved the package. The same could happen in reverse, when a catalog entry moved a package an override pins. Both cases now re-resolve instead.

- `pnpm install` now updates the lockfile in place even when several kinds of changes happened since the last install — for example a removed dependency together with a widened `ignoredOptionalDependencies` list, or a dependency edit alongside a patch or settings change. Previously any combination of changes forced a full re-resolution [#13763](https://github.com/pnpm/pnpm/issues/13763).

- Removing the last dependency that references a catalog entry via the fast lockfile update no longer leaves the stale catalog entry in `pnpm-lock.yaml`.

- `pnpm install` after moving a dependency between `dependencies`, `devDependencies`, and `optionalDependencies` now updates the lockfile in place instead of re-resolving the whole dependency graph [#13696](https://github.com/pnpm/pnpm/issues/13696).

- Widening a dependency's range no longer leaves the project on an older version. The lockfile update now points the project at the highest version of that dependency already in the lockfile that satisfies the new range — matching what a full resolution records — instead of keeping the locked version whenever it happened to satisfy, which could leave a duplicate behind. A range change that only an already-locked version satisfies is now also handled without re-resolving [#13778](https://github.com/pnpm/pnpm/issues/13778).

- Adding a package to a workspace no longer forces a full re-resolution when every dependency it declares is already locked for a sibling. The lockfile update writes the new project's importer entry from the versions the lockfile already holds; a dependency no locked version satisfies still reaches the resolver [#13696](https://github.com/pnpm/pnpm/issues/13696).

- Changing a `pnpm.overrides` entry to a version range now updates the lockfile in place when a version the lockfile already holds satisfies the range, instead of re-resolving the whole dependency graph. Only exact versions were handled before [#13696](https://github.com/pnpm/pnpm/issues/13696).

- Changing a parent-scoped `pnpm.overrides` entry (`"parent>child": "2.0.0"`) now updates the lockfile in place instead of re-resolving the whole dependency graph. Only the named parent's dependency moves; every other package keeps the version it had [#13795](https://github.com/pnpm/pnpm/issues/13795).

- Removing a dependency, or moving one to another already-locked version, no longer re-resolves the whole dependency graph just because some package resolves a peer with the same name. The lockfile update now compares the peer suffixes against the exact `name@version` the removal severed, so a suffix that names a different — still present — version of that dependency is left alone [#13781](https://github.com/pnpm/pnpm/issues/13781).

- Projects with a pnpmfile now use the fast lockfile update paths: an unchanged pnpmfile (proven by the recorded `pnpmfileChecksum`) no longer forces a full re-resolution for removals, dependency group moves, compatible range changes, and the other in-place lockfile rewrites [#13696](https://github.com/pnpm/pnpm/issues/13696).

- `pnpm remove` no longer re-resolves the dependency graph. The removed dependency's entries are dropped from `pnpm-lock.yaml` and anything they made unreachable is pruned, without registry access. The install still falls back to a full resolution when a surviving package resolves a peer dependency through the removed one.

- Removing a package from a workspace no longer forces a full re-resolution. The lockfile update drops the departed project's importer entry and prunes whatever only it depended on. A project that is still linked from a surviving project continues to be reported as an error [#13696](https://github.com/pnpm/pnpm/issues/13696).

- Projects using `resolutionMode: time-based` now benefit from the fast lockfile update paths. A removal, a dependency group move, or a compatible range change no longer forces a full re-resolution just because the lockfile carries a `time` field [#13696](https://github.com/pnpm/pnpm/issues/13696).

- An install that drops the last dependent of a patched package no longer updates the lockfile in place and succeeds silently. Removing a dependency, widening `ignoredOptionalDependencies`, or adding a removal override could each prune the package while the patch stayed configured; such an install now falls back to a full resolution, which reports the unused patch with `ERR_PNPM_UNUSED_PATCH`. Under `allowUnusedPatches`, where the lockfile update is kept, the same install now warns that the patch went unused instead of saying nothing [#13827](https://github.com/pnpm/pnpm/issues/13827).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.27
  - @pnpm/bins.remover@1100.0.20
  - @pnpm/building.after-install@1102.0.17
  - @pnpm/building.during-install@1102.0.16
  - @pnpm/building.policy@1100.0.19
  - @pnpm/catalogs.config@1100.0.5
  - @pnpm/config.parse-overrides@1100.1.3
  - @pnpm/config.version-policy@1100.2.0
  - @pnpm/deps.graph-hasher@1100.2.17
  - @pnpm/error@1100.1.2
  - @pnpm/exec.lifecycle@1100.1.13
  - @pnpm/hooks.read-package-hook@1100.2.4
  - @pnpm/installing.context@1100.1.2
  - @pnpm/installing.deps-resolver@1101.1.2
  - @pnpm/installing.deps-restorer@1102.3.2
  - @pnpm/installing.linking.hoist@1100.0.27
  - @pnpm/installing.linking.modules-cleaner@1100.1.19
  - @pnpm/installing.package-requester@1102.1.10
  - @pnpm/lockfile.filtering@1100.2.3
  - @pnpm/lockfile.fs@1100.2.2
  - @pnpm/lockfile.preferred-versions@1100.0.29
  - @pnpm/lockfile.settings-checker@1100.2.1
  - @pnpm/lockfile.to-pnp@1100.1.14
  - @pnpm/lockfile.utils@1101.1.0
  - @pnpm/lockfile.verification@1100.1.0
  - @pnpm/network.auth-header@1101.1.10
  - @pnpm/patching.config@1100.1.1
  - @pnpm/pkg-manifest.utils@1100.4.0
  - @pnpm/pnpr.client@1.3.12
  - @pnpm/store.controller-types@1101.1.1
  - @pnpm/store.index@1100.2.4
  - @pnpm/workspace.project-manifest-reader@1100.0.24
