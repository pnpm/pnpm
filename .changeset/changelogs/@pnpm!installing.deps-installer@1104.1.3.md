## 1104.1.3

### Patch Changes

- Fixed `pnpm dedupe` requiring a second pass after bumping a direct dependency in `package.json` [pnpm/pnpm#14987](https://github.com/pnpm/pnpm/issues/14987).

- `pnpm install --force` now removes obsolete dependency links inside virtual-store packages when their dependencies change. Invalid dependency names are ignored during obsolete-link cleanup [#15039](https://github.com/pnpm/pnpm/issues/15039).

- `pnpm install --prod` no longer downloads the registry packages that only a devDependency reaches [#881](https://github.com/pnpm/pnpm/issues/881).

- `pnpm update` and `pnpm audit --fix=update` no longer copy dependencies added by `packageExtensions`, a `readPackage` hook, or an override into `package.json`. Those dependencies keep the specifier the hook or override gives them. `pnpm update --latest` no longer resolves past that specifier. `pnpm audit --fix=update` now warns when one of them pins a vulnerable version. The warning points at `pnpm audit --fix` [#14928](https://github.com/pnpm/pnpm/issues/14928).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.32
  - @pnpm/bins.remover@1100.0.24
  - @pnpm/building.after-install@1103.0.5
  - @pnpm/building.during-install@1102.2.2
  - @pnpm/building.policy@1100.1.2
  - @pnpm/core-loggers@1101.0.0
  - @pnpm/crypto.hash@1100.0.5
  - @pnpm/deps.graph-hasher@1100.3.3
  - @pnpm/deps.path@1101.0.3
  - @pnpm/exec.lifecycle@1100.1.18
  - @pnpm/fs.symlink-dependency@1100.0.20
  - @pnpm/hooks.read-package-hook@1100.3.3
  - @pnpm/hooks.types@1101.0.3
  - @pnpm/installing.context@1101.0.5
  - @pnpm/installing.deps-resolver@1102.2.2
  - @pnpm/installing.deps-restorer@1103.1.3
  - @pnpm/installing.linking.direct-dep-linker@1100.0.20
  - @pnpm/installing.linking.hoist@1100.0.32
  - @pnpm/installing.linking.modules-cleaner@1100.1.24
  - @pnpm/installing.modules-yaml@1101.0.3
  - @pnpm/installing.package-requester@1102.1.15
  - @pnpm/lockfile.filtering@1100.2.8
  - @pnpm/lockfile.fs@1100.2.8
  - @pnpm/lockfile.preferred-versions@1100.0.34
  - @pnpm/lockfile.pruner@1100.0.24
  - @pnpm/lockfile.settings-checker@1100.2.7
  - @pnpm/lockfile.to-pnp@1101.0.5
  - @pnpm/lockfile.utils@1102.1.3
  - @pnpm/lockfile.verification@1100.1.6
  - @pnpm/lockfile.walker@1100.0.24
  - @pnpm/patching.config@1100.1.6
  - @pnpm/pkg-manifest.utils@1100.4.5
  - @pnpm/pnpr.client@3.0.3
  - @pnpm/resolving.local-resolver@1101.2.2
  - @pnpm/resolving.resolver-base@1101.3.0
  - @pnpm/store.controller-types@1101.3.0
  - @pnpm/workspace.project-manifest-reader@1100.0.29
