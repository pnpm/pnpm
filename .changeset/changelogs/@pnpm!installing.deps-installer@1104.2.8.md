## 1104.2.8

### Patch Changes

- With `injectWorkspacePackages` and per-project lockfiles, `pnpm install` now updates a project's lockfile when a local tarball that an injected workspace project depends on is replaced. The project's lockfile kept the old version and integrity before.

- The error for a dependency with an invalid name in a project's manifest now names the project's directory. It used to say only "The current package", which did not tell which workspace project to fix [#17475](https://github.com/pnpm/pnpm/issues/17475).

- `pnpm install` and `pnpm rebuild` now refresh privately hoisted `.bin` shims in `node_modules/.pnpm/node_modules/.bin` after a dependency build replaces a script target with a native executable [#17402](https://github.com/pnpm/pnpm/issues/17402).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.40
  - @pnpm/building.after-install@1103.0.14
  - @pnpm/building.during-install@1102.2.11
  - @pnpm/deps.graph-hasher@1100.3.10
  - @pnpm/exec.lifecycle@1100.1.27
  - @pnpm/installing.context@1101.0.14
  - @pnpm/installing.deps-resolver@1102.2.11
  - @pnpm/installing.deps-restorer@1103.2.8
  - @pnpm/installing.linking.hoist@1100.0.41
  - @pnpm/installing.linking.modules-cleaner@1100.1.32
  - @pnpm/installing.package-requester@1102.2.8
  - @pnpm/lockfile.filtering@1100.2.15
  - @pnpm/lockfile.fs@1100.2.16
  - @pnpm/lockfile.preferred-versions@1100.0.41
  - @pnpm/lockfile.settings-checker@1100.2.16
  - @pnpm/lockfile.to-pnp@1101.0.13
  - @pnpm/lockfile.utils@1102.1.9
  - @pnpm/lockfile.verification@1100.1.15
  - @pnpm/pnpr.client@3.1.6
