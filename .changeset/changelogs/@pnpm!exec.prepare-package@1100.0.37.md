## 1100.0.37

### Patch Changes

- `pnpm install` now installs git-hosted dependencies without preparing them when their builds are explicitly denied by `allowBuilds`. Dependencies that require preparation still need an explicit allow or deny decision [pnpm/pnpm#10522](https://github.com/pnpm/pnpm/issues/10522).

- Updated dependencies:
  - @pnpm/exec.lifecycle@1100.1.18
