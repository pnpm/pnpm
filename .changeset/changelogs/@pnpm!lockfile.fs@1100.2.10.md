## 1100.2.10

### Patch Changes

- `pnpm install --frozen-lockfile` now works on a detached HEAD when `gitBranchLockfile` is enabled. The install now reads the lockfiles of the local and remote-tracking branches that contain the checked-out commit. It still writes the shared `pnpm-lock.yaml` [#7672](https://github.com/pnpm/pnpm/issues/7672).

- `pnpm install` now repairs a `pnpm-lock.yaml` whose `(patch_hash=<hash>)` dependency paths disagree with its `patchedDependencies` map, including paths that lack the hash their patch calls for. pnpm previously accepted such a lockfile as up to date and kept the old patched files. `pnpm install --frozen-lockfile` now fails on such a lockfile with `ERR_PNPM_INCONSISTENT_PATCH_HASH`. It fails with `ERR_PNPM_UNCHECKABLE_PATCH_HASH` when a patch hash in the lockfile is malformed, or when the lockfile lacks the package version or patch entry that the check needs [#15336](https://github.com/pnpm/pnpm/pull/15336).

- pnpm no longer reports `pnpm-lock.yaml` as broken when a project depends on a package named `constructor`. A `__proto__` key in the lockfile is now kept as a plain entry when pnpm reads or writes the lockfile. It no longer replaces the prototype of the objects pnpm builds from it [#11028](https://github.com/pnpm/pnpm/issues/11028).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/deps.path@1101.0.5
  - @pnpm/error@1100.2.1
  - @pnpm/fs.graceful-fs@1100.2.4
  - @pnpm/lockfile.merger@1100.0.25
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/network.git-utils@1100.0.6
  - @pnpm/patching.config@1100.1.8
