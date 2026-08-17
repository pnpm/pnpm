## 1102.0.16

### Patch Changes

- Fixed intermittent `ERR_PNPM_ENOENT` and `ERR_PNPM_ENOTEMPTY` errors while renaming `_tmp_*` directories during installation with `nodeLinker: hoisted`, in workspaces that also use `patchedDependencies`.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.27
  - @pnpm/config.reader@1101.17.0
  - @pnpm/deps.graph-hasher@1100.2.17
  - @pnpm/error@1100.1.2
  - @pnpm/exec.lifecycle@1100.1.13
  - @pnpm/patching.apply-patch@1100.0.6
  - @pnpm/pkg-manifest.reader@1100.0.16
  - @pnpm/store.controller-types@1101.1.1
