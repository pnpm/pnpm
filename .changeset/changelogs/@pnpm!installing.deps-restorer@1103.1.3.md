## 1103.1.3

### Patch Changes

- `pnpm install --force` now removes obsolete dependency links inside virtual-store packages when their dependencies change. Invalid dependency names are ignored during obsolete-link cleanup [#15039](https://github.com/pnpm/pnpm/issues/15039).

- The install summary now names the version each dependency resolved to when `node-linker` is `hoisted`. It also lists what an install restores after `node_modules` is deleted, and both sides of a version change. The summary showed the range recorded in `package.json`, or nothing at all [#15161](https://github.com/pnpm/pnpm/issues/15161).

- `pnpm install --prod` no longer downloads the registry packages that only a devDependency reaches [#881](https://github.com/pnpm/pnpm/issues/881).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.32
  - @pnpm/building.during-install@1102.2.2
  - @pnpm/building.policy@1100.1.2
  - @pnpm/config.package-is-installable@1100.1.7
  - @pnpm/core-loggers@1101.0.0
  - @pnpm/deps.graph-builder@1101.0.5
  - @pnpm/deps.graph-hasher@1100.3.3
  - @pnpm/deps.path@1101.0.3
  - @pnpm/exec.lifecycle@1100.1.18
  - @pnpm/fs.symlink-dependency@1100.0.20
  - @pnpm/installing.linking.direct-dep-linker@1100.0.20
  - @pnpm/installing.linking.hoist@1100.0.32
  - @pnpm/installing.linking.modules-cleaner@1100.1.24
  - @pnpm/installing.linking.real-hoist@1100.1.19
  - @pnpm/installing.modules-yaml@1101.0.3
  - @pnpm/installing.package-requester@1102.1.15
  - @pnpm/lockfile.filtering@1100.2.8
  - @pnpm/lockfile.fs@1100.2.8
  - @pnpm/lockfile.to-pnp@1101.0.5
  - @pnpm/lockfile.utils@1102.1.3
  - @pnpm/patching.config@1100.1.6
  - @pnpm/pnpr.client@3.0.3
  - @pnpm/store.controller-types@1101.3.0
  - @pnpm/workspace.project-manifest-reader@1100.0.29
