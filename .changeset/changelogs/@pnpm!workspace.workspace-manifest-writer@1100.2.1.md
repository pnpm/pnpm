## 1100.2.1

### Patch Changes

- pnpm now preserves scalar YAML anchors and aliases when editing `pnpm-workspace.yaml`. Removing the entry that defines an anchor keeps surviving aliases valid. Entries updated to different values are written separately [#8245](https://github.com/pnpm/pnpm/issues/8245).

- Updated dependencies:
  - @pnpm/building.policy@1100.1.2
  - @pnpm/lockfile.types@1100.1.2
  - @pnpm/yaml.document-sync@1100.0.2
