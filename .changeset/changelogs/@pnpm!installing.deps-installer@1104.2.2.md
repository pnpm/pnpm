## 1104.2.2

### Patch Changes

- `pnpm add constructor` now adds the `constructor` package like any other dependency. pnpm used to read built-in object properties as its previous specifier and dependency type.

- A custom resolver's `shouldRefreshResolution` hook that rejects no longer crashes pnpm with an unhandled rejection when another hook has already asked for a refresh.

- `pnpm install` no longer crashes or writes a wrong lockfile when a dependency, peer dependency, or catalog entry is named `constructor`.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.35
  - @pnpm/building.after-install@1103.0.8
  - @pnpm/building.during-install@1102.2.5
  - @pnpm/exec.lifecycle@1100.1.21
  - @pnpm/installing.context@1101.0.8
  - @pnpm/installing.deps-resolver@1102.2.5
  - @pnpm/installing.deps-restorer@1103.2.2
  - @pnpm/installing.linking.hoist@1100.0.35
  - @pnpm/installing.linking.modules-cleaner@1100.1.27
  - @pnpm/installing.package-requester@1102.2.2
  - @pnpm/lockfile.fs@1100.2.11
  - @pnpm/lockfile.settings-checker@1100.2.10
  - @pnpm/lockfile.to-pnp@1101.0.8
  - @pnpm/lockfile.verification@1100.1.9
  - @pnpm/pnpr.client@3.1.1
  - @pnpm/resolving.npm-resolver@1104.2.3
  - @pnpm/store.index@1101.0.0
