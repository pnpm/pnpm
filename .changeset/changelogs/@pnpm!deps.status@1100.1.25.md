## 1100.1.25

### Patch Changes

- `pnpm run` no longer reinstalls dependencies when a `node_modules` directory installed outside CI is used with `CI=true`, or the other way around [#12337](https://github.com/pnpm/pnpm/issues/12337).

- `pnpm install` now repairs a `pnpm-lock.yaml` whose `(patch_hash=<hash>)` dependency paths disagree with its `patchedDependencies` map, including paths that lack the hash their patch calls for. pnpm previously accepted such a lockfile as up to date and kept the old patched files. `pnpm install --frozen-lockfile` now fails on such a lockfile with `ERR_PNPM_INCONSISTENT_PATCH_HASH`. It fails with `ERR_PNPM_UNCHECKABLE_PATCH_HASH` when a patch hash in the lockfile is malformed, or when the lockfile lacks the package version or patch entry that the check needs [#15336](https://github.com/pnpm/pnpm/pull/15336).

- With `nodeLinker: hoisted`, `pnpm install` now restores a workspace project's `node_modules` after it was deleted. Before, the install printed "Already up to date" and left the project without the dependencies nested under it.

- `pnpm install` now fails with `ERR_PNPM_PATCH_NOT_FOUND` when a patch file listed in `patchedDependencies` does not exist. It used to fail with a raw `ENOENT` error and a stack trace [#5268](https://github.com/pnpm/pnpm/issues/5268).

- A repeat install now keeps the fast path when a declared local file dependency is replaced by an override [pnpm/pnpm#12892](https://github.com/pnpm/pnpm/issues/12892).

- `pnpm install` refreshes injected copies of workspace packages when source projects are rebuilt. Injected copies previously stayed stale until `pnpm install --force` [pnpm/pnpm#4407](https://github.com/pnpm/pnpm/issues/4407).

- When `verifyDepsBeforeRun` triggers an install before a filtered `pnpm run` or `pnpm exec`, pnpm now installs only the selected projects and their dependencies. A later filtered command also installs a selected project that an earlier filtered install skipped [#11865](https://github.com/pnpm/pnpm/issues/11865).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/catalogs.resolver@1100.1.1
  - @pnpm/config.parse-overrides@1100.1.7
  - @pnpm/config.reader@1102.3.1
  - @pnpm/error@1100.2.1
  - @pnpm/hooks.read-package-hook@1100.3.5
  - @pnpm/installing.context@1101.0.7
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/lockfile.settings-checker@1100.2.9
  - @pnpm/lockfile.verification@1100.1.8
  - @pnpm/resolving.resolver-base@1101.3.2
  - @pnpm/workspace.projects-reader@1101.1.1
  - @pnpm/workspace.state@1100.0.46
  - @pnpm/workspace.workspace-manifest-reader@1100.2.1
