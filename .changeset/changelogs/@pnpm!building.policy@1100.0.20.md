## 1100.0.20

### Patch Changes

- `pnpm remove` now prunes undecided entries (`"set this to true or false"`) from `allowBuilds` in `pnpm-workspace.yaml` when `sharedWorkspaceLockfile: true` and the corresponding packages are removed [pnpm/pnpm#13892](https://github.com/pnpm/pnpm/issues/13892).

- Updated dependencies:
  - @pnpm/config.version-policy@1100.2.1
  - @pnpm/deps.path@1101.0.0
  - @pnpm/types@1102.0.0
