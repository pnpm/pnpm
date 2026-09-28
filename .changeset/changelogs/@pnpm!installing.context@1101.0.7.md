## 1101.0.7

### Patch Changes

- `pnpm install` now repairs a `pnpm-lock.yaml` whose `(patch_hash=<hash>)` dependency paths disagree with its `patchedDependencies` map, including paths that lack the hash their patch calls for. pnpm previously accepted such a lockfile as up to date and kept the old patched files. `pnpm install --frozen-lockfile` now fails on such a lockfile with `ERR_PNPM_INCONSISTENT_PATCH_HASH`. It fails with `ERR_PNPM_UNCHECKABLE_PATCH_HASH` when a patch hash in the lockfile is malformed, or when the lockfile lacks the package version or patch entry that the check needs [#15336](https://github.com/pnpm/pnpm/pull/15336).

- pnpm's built-in package compatibility database no longer applies to a project's own manifest. A project named like a published package, such as `vue-loader`, no longer gains dependencies on `pnpm install` or `pnpm update`. User-configured `packageExtensions` still apply to project manifests [#11700](https://github.com/pnpm/pnpm/issues/11700).

- Updated dependencies:
  - @pnpm/error@1100.2.1
  - @pnpm/installing.modules-yaml@1101.0.5
  - @pnpm/installing.read-projects-context@1101.0.7
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/lockfile.pruner@1100.0.26
  - @pnpm/resolving.resolver-base@1101.3.2
  - @pnpm/store.controller@1102.2.1
