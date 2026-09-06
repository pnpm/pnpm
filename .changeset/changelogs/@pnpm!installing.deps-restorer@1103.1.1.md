## 1103.1.1

### Patch Changes

- Made downloaded runtimes available to dependency lifecycle scripts during installation.

- `pnpm install --node-linker=hoisted` no longer downloads every optional dependency it reports as skipped when `node_modules` already exists. Those downloads also continued after pnpm printed `Done` [#14139](https://github.com/pnpm/pnpm/issues/14139).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.30
  - @pnpm/building.during-install@1102.2.0
  - @pnpm/building.policy@1100.1.0
  - @pnpm/config.package-is-installable@1100.1.6
  - @pnpm/deps.graph-builder@1101.0.3
  - @pnpm/deps.graph-hasher@1100.3.1
  - @pnpm/error@1100.1.4
  - @pnpm/exec.lifecycle@1100.1.16
  - @pnpm/installing.linking.hoist@1100.0.30
  - @pnpm/installing.linking.modules-cleaner@1100.1.22
  - @pnpm/installing.linking.real-hoist@1100.1.17
  - @pnpm/installing.package-requester@1102.1.13
  - @pnpm/lockfile.filtering@1100.2.6
  - @pnpm/lockfile.fs@1100.2.6
  - @pnpm/lockfile.to-pnp@1101.0.3
  - @pnpm/lockfile.utils@1102.1.1
  - @pnpm/patching.config@1100.1.4
  - @pnpm/pkg-manifest.reader@1100.0.19
  - @pnpm/pnpr.client@3.0.1
  - @pnpm/workspace.project-manifest-reader@1100.0.27
