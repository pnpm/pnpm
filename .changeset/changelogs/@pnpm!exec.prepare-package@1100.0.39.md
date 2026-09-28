## 1100.0.39

### Patch Changes

- Installing a git-hosted dependency that has to be built no longer fails when that dependency's own dependencies have build scripts nobody approved. pnpm skips those builds while preparing the dependency, as it does without `strictDepBuilds` [#9764](https://github.com/pnpm/pnpm/issues/9764).

- pnpm now uses pnpm to prepare a git-hosted dependency that is a pnpm workspace without a committed lockfile. It used npm before, which could skip the dependency's build [#14011](https://github.com/pnpm/pnpm/issues/14011).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/error@1100.2.1
  - @pnpm/exec.lifecycle@1100.1.20
  - @pnpm/pkg-manifest.reader@1100.0.21
