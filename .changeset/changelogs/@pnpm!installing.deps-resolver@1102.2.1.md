## 1102.2.1

### Patch Changes

- A `readPackage` hook that edits its argument in place no longer changes what a later install in the same command resolves. A `deprecated` notice read from the lockfile no longer carries over to another install either [#13988](https://github.com/pnpm/pnpm/issues/13988).

- `pnpm install` now auto-installs missing transitive peers when workspace projects share a dependency at different depths. This also removes incomplete duplicate peer contexts from the lockfile. Fixes [pnpm/pnpm#14840](https://github.com/pnpm/pnpm/issues/14840).

- Updated dependencies:
  - @pnpm/deps.graph-hasher@1100.3.2
  - @pnpm/deps.path@1101.0.2
  - @pnpm/deps.peer-range@1100.1.2
  - @pnpm/lockfile.preferred-versions@1100.0.33
  - @pnpm/lockfile.pruner@1100.0.23
  - @pnpm/lockfile.utils@1102.1.2
  - @pnpm/patching.config@1100.1.5
  - @pnpm/pkg-manifest.utils@1100.4.4
  - @pnpm/resolving.npm-resolver@1104.1.2
