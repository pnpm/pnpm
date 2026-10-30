## 1103.2.8

### Patch Changes

- `pnpm install` and `pnpm rebuild` now refresh privately hoisted `.bin` shims in `node_modules/.pnpm/node_modules/.bin` after a dependency build replaces a script target with a native executable [#17402](https://github.com/pnpm/pnpm/issues/17402).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.40
  - @pnpm/building.during-install@1102.2.11
  - @pnpm/deps.graph-builder@1101.1.8
  - @pnpm/deps.graph-hasher@1100.3.10
  - @pnpm/exec.lifecycle@1100.1.27
  - @pnpm/installing.linking.hoist@1100.0.41
  - @pnpm/installing.linking.modules-cleaner@1100.1.32
  - @pnpm/installing.linking.real-hoist@1100.1.25
  - @pnpm/installing.package-requester@1102.2.8
  - @pnpm/lockfile.filtering@1100.2.15
  - @pnpm/lockfile.fs@1100.2.16
  - @pnpm/lockfile.to-pnp@1101.0.13
  - @pnpm/lockfile.utils@1102.1.9
  - @pnpm/pnpr.client@3.1.6
