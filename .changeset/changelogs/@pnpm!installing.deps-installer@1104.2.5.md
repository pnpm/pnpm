## 1104.2.5

### Patch Changes

- `catalogPrune` no longer removes catalog entries that `pnpm-lock.yaml` still records for workspace projects missing from disk. This affected `pnpm remove` and installs that leave the lockfile unchanged, such as frozen installs. Before, a following frozen install failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` [#16679](https://github.com/pnpm/pnpm/issues/16679).

- `pnpm install --fix-lockfile` no longer removes the `deprecated` and `hasBin` fields from lockfile entries [#6600](https://github.com/pnpm/pnpm/issues/6600).

- `pnpm dedupe` now preserves a hoisted workspace package when another package uses a separately installed peer with the same name [pnpm/pnpm#16935](https://github.com/pnpm/pnpm/issues/16935).

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.11
  - @pnpm/building.during-install@1102.2.8
  - @pnpm/building.policy@1100.1.7
  - @pnpm/config.version-policy@1100.2.8
  - @pnpm/exec.lifecycle@1100.1.24
  - @pnpm/hooks.read-package-hook@1100.3.8
  - @pnpm/installing.context@1101.0.11
  - @pnpm/installing.deps-resolver@1102.2.8
  - @pnpm/installing.deps-restorer@1103.2.5
  - @pnpm/installing.linking.hoist@1100.0.38
  - @pnpm/installing.package-requester@1102.2.5
  - @pnpm/lockfile.settings-checker@1100.2.13
  - @pnpm/lockfile.verification@1100.1.12
  - @pnpm/resolving.npm-resolver@1104.2.6
  - @pnpm/store.index@1101.0.2
