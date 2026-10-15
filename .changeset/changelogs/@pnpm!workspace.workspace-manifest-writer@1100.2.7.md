## 1100.2.7

### Patch Changes

- `catalogPrune` no longer removes catalog entries that `pnpm-lock.yaml` still records for workspace projects missing from disk. This affected `pnpm remove` and installs that leave the lockfile unchanged, such as frozen installs. Before, a following frozen install failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` [#16679](https://github.com/pnpm/pnpm/issues/16679).

- Updated dependencies:
  - @pnpm/building.policy@1100.1.7
  - @pnpm/config.version-policy@1100.2.8
