## 1100.1.23

### Patch Changes

- pnpm now recognizes `externalDependencies` and `autoInstallPeersFromHighestMatch` in `pnpm-workspace.yaml` without warning and forwards both settings to `pnpm install`. Previously, setting `externalDependencies` in `pnpm-workspace.yaml` with `nodeLinker: hoisted` crashed `pnpm install` with `externalDependencies.has is not a function` [pnpm/tasks#166](https://github.com/pnpm/tasks/issues/166).

- Updated dependencies:
  - @pnpm/deps.path@1101.0.7
  - @pnpm/lockfile.types@1100.1.5
  - @pnpm/lockfile.utils@1102.1.7
