## 1104.1.1

### Patch Changes

- Made downloaded runtimes available to dependency lifecycle scripts during installation.

- `catalogMode` and `--save-catalog` no longer move a local path, tarball, or `workspace:<path>` specifier into a catalog. Such a specifier is resolved against the project that declares it, so one catalog entry cannot mean the same directory for every project that references it [#14437](https://github.com/pnpm/pnpm/issues/14437).

- `pnpm install` now relinks workspace packages when `publishConfig.linkDirectory` changes. Frozen installs report an outdated lockfile until it is regenerated [pnpm/pnpm#14488](https://github.com/pnpm/pnpm/issues/14488).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.30
  - @pnpm/bins.remover@1100.0.23
  - @pnpm/building.after-install@1103.0.3
  - @pnpm/building.during-install@1102.2.0
  - @pnpm/building.policy@1100.1.0
  - @pnpm/catalogs.config@1100.0.7
  - @pnpm/catalogs.resolver@1100.1.0
  - @pnpm/config.parse-overrides@1100.1.5
  - @pnpm/config.version-policy@1100.2.3
  - @pnpm/deps.graph-hasher@1100.3.1
  - @pnpm/error@1100.1.4
  - @pnpm/exec.lifecycle@1100.1.16
  - @pnpm/hooks.read-package-hook@1100.3.1
  - @pnpm/hooks.types@1101.0.2
  - @pnpm/installing.context@1101.0.3
  - @pnpm/installing.deps-resolver@1102.2.0
  - @pnpm/installing.deps-restorer@1103.1.1
  - @pnpm/installing.linking.hoist@1100.0.30
  - @pnpm/installing.linking.modules-cleaner@1100.1.22
  - @pnpm/installing.package-requester@1102.1.13
  - @pnpm/lockfile.filtering@1100.2.6
  - @pnpm/lockfile.fs@1100.2.6
  - @pnpm/lockfile.preferred-versions@1100.0.32
  - @pnpm/lockfile.pruner@1100.0.22
  - @pnpm/lockfile.settings-checker@1100.2.5
  - @pnpm/lockfile.to-pnp@1101.0.3
  - @pnpm/lockfile.utils@1102.1.1
  - @pnpm/lockfile.verification@1100.1.4
  - @pnpm/lockfile.walker@1100.0.22
  - @pnpm/network.auth-header@1101.1.13
  - @pnpm/patching.config@1100.1.4
  - @pnpm/pkg-manifest.utils@1100.4.3
  - @pnpm/pnpr.client@3.0.1
  - @pnpm/resolving.local-resolver@1101.2.0
  - @pnpm/store.index@1100.3.1
  - @pnpm/workspace.project-manifest-reader@1100.0.27
