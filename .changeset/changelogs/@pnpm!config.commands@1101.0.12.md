## 1101.0.12

### Patch Changes

- pnpm now recognizes `externalDependencies` and `autoInstallPeersFromHighestMatch` in `pnpm-workspace.yaml` without warning and forwards both settings to `pnpm install`. Previously, setting `externalDependencies` in `pnpm-workspace.yaml` with `nodeLinker: hoisted` crashed `pnpm install` with `externalDependencies.has is not a function` [pnpm/tasks#166](https://github.com/pnpm/tasks/issues/166).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.33
  - @pnpm/config.normalize-registries@1101.0.4
  - @pnpm/config.reader@1102.3.6
  - @pnpm/workspace.workspace-manifest-writer@1100.2.8
