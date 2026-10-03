## 1102.2.6

### Patch Changes

- When an optional dependency fails to build, pnpm now removes its link from `node_modules`. A repeat `pnpm install` then reports "Already up to date" and no longer reruns the failing build [#16468](https://github.com/pnpm/pnpm/issues/16468).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.36
  - @pnpm/config.package-is-installable@1100.2.2
  - @pnpm/config.reader@1102.3.3
  - @pnpm/core-loggers@1101.0.2
  - @pnpm/deps.graph-hasher@1100.3.6
  - @pnpm/exec.lifecycle@1100.1.22
  - @pnpm/pnpr.client@3.1.2
  - @pnpm/store.controller-types@1101.3.3
