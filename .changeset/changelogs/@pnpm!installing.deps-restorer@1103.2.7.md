## 1103.2.7

### Patch Changes

- With `nodeLinker: hoisted`, an install that stops partway no longer leaves the next install trusting the packages it was replacing. Before, reverting the change that started the stopped install, such as a patch edit, kept the half-replaced package in `node_modules`.

- `pnpm install` now removes command shims from `node_modules/.bin` when a workspace dependency renames or removes an entry from its `bin` field [pnpm/tasks#211](https://github.com/pnpm/tasks/issues/211).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.39
  - @pnpm/bins.remover@1100.0.30
  - @pnpm/building.during-install@1102.2.10
  - @pnpm/building.policy@1100.1.9
  - @pnpm/config.normalize-registries@1101.0.5
  - @pnpm/config.package-is-installable@1100.2.5
  - @pnpm/core-loggers@1101.0.4
  - @pnpm/deps.graph-builder@1101.1.7
  - @pnpm/deps.graph-hasher@1100.3.9
  - @pnpm/deps.path@1101.0.8
  - @pnpm/exec.lifecycle@1100.1.26
  - @pnpm/fs.symlink-dependency@1100.0.26
  - @pnpm/installing.linking.direct-dep-linker@1100.0.26
  - @pnpm/installing.linking.hoist@1100.0.40
  - @pnpm/installing.linking.modules-cleaner@1100.1.31
  - @pnpm/installing.linking.real-hoist@1100.1.24
  - @pnpm/installing.modules-yaml@1101.0.8
  - @pnpm/installing.package-requester@1102.2.7
  - @pnpm/lockfile.filtering@1100.2.14
  - @pnpm/lockfile.fs@1100.2.15
  - @pnpm/lockfile.to-pnp@1101.0.12
  - @pnpm/lockfile.utils@1102.1.8
  - @pnpm/patching.config@1100.1.11
  - @pnpm/pkg-manifest.reader@1100.0.24
  - @pnpm/pnpr.client@3.1.5
  - @pnpm/store.controller-types@1101.3.5
  - @pnpm/types@1102.1.3
  - @pnpm/workspace.project-manifest-reader@1100.1.5
