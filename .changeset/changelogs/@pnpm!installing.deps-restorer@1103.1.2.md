## 1103.1.2

### Patch Changes

- pnpm now writes `node_modules/.package-map.json` only when `nodeExperimentalPackageMap` is enabled. Nothing reads the file without that setting. An install that stops writing the map removes the one a previous install left.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.31
  - @pnpm/building.during-install@1102.2.1
  - @pnpm/building.policy@1100.1.1
  - @pnpm/deps.graph-builder@1101.0.4
  - @pnpm/deps.graph-hasher@1100.3.2
  - @pnpm/deps.path@1101.0.2
  - @pnpm/exec.lifecycle@1100.1.17
  - @pnpm/installing.linking.hoist@1100.0.31
  - @pnpm/installing.linking.modules-cleaner@1100.1.23
  - @pnpm/installing.linking.real-hoist@1100.1.18
  - @pnpm/installing.modules-yaml@1101.0.2
  - @pnpm/installing.package-requester@1102.1.14
  - @pnpm/lockfile.filtering@1100.2.7
  - @pnpm/lockfile.fs@1100.2.7
  - @pnpm/lockfile.to-pnp@1101.0.4
  - @pnpm/lockfile.utils@1102.1.2
  - @pnpm/patching.config@1100.1.5
  - @pnpm/pnpr.client@3.0.2
  - @pnpm/workspace.project-manifest-reader@1100.0.28
