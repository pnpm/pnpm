## 1100.0.2

### Patch Changes

- pnpm now preserves scalar YAML anchors and aliases when editing `pnpm-workspace.yaml`. Removing the entry that defines an anchor keeps surviving aliases valid. Entries updated to different values are written separately [#8245](https://github.com/pnpm/pnpm/issues/8245).

- pnpm now preserves comments and existing key order when updating `package.yaml`. New keys are appended to their mapping [pnpm/pnpm#2008](https://github.com/pnpm/pnpm/issues/2008).
