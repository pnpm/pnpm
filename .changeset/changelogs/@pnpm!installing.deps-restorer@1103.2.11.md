## 1103.2.11

### Patch Changes

- `pnpm install` with `nodeLinker: hoisted` no longer counts missing packages as removed. This fixes inflated removal counts for skipped optional dependencies [pnpm/pnpm#18178](https://github.com/pnpm/pnpm/issues/18178).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.43
  - @pnpm/bins.remover@1100.0.33
  - @pnpm/building.during-install@1102.2.14
  - @pnpm/building.policy@1100.1.11
  - @pnpm/config.normalize-registries@1101.0.6
  - @pnpm/config.package-is-installable@1100.2.7
  - @pnpm/core-loggers@1101.0.5
  - @pnpm/deps.graph-builder@1101.1.11
  - @pnpm/deps.graph-hasher@1100.3.13
  - @pnpm/deps.path@1101.0.10
  - @pnpm/exec.lifecycle@1100.1.30
  - @pnpm/fs.symlink-dependency@1100.0.27
  - @pnpm/installing.linking.direct-dep-linker@1100.0.27
  - @pnpm/installing.linking.hoist@1100.0.44
  - @pnpm/installing.linking.modules-cleaner@1100.1.35
  - @pnpm/installing.linking.real-hoist@1100.1.28
  - @pnpm/installing.modules-yaml@1101.0.10
  - @pnpm/installing.package-requester@1102.2.11
  - @pnpm/lockfile.filtering@1100.2.18
  - @pnpm/lockfile.fs@1100.2.19
  - @pnpm/lockfile.to-pnp@1101.0.16
  - @pnpm/lockfile.utils@1102.1.12
  - @pnpm/patching.config@1100.1.13
  - @pnpm/pkg-manifest.reader@1100.0.25
  - @pnpm/pnpr.client@3.1.9
  - @pnpm/resolving.npm-resolver@1104.2.11
  - @pnpm/resolving.resolver-base@1101.3.6
  - @pnpm/store.controller-types@1101.3.7
  - @pnpm/types@1102.1.4
  - @pnpm/workspace.project-manifest-reader@1100.1.7
  - @pnpm/workspace.state@1100.0.56
