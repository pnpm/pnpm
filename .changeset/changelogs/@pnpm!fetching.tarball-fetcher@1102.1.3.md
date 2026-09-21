## 1102.1.3

### Patch Changes

- `pnpm install` now installs git-hosted dependencies without preparing them when their builds are explicitly denied by `allowBuilds`. Dependencies that require preparation still need an explicit allow or deny decision [pnpm/pnpm#10522](https://github.com/pnpm/pnpm/issues/10522).

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.0
  - @pnpm/exec.prepare-package@1100.0.37
  - @pnpm/fetching.fetcher-base@1100.2.10
  - @pnpm/fs.graceful-fs@1100.2.2
