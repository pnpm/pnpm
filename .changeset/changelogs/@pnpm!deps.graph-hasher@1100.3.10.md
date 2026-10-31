## 1100.3.10

### Patch Changes

- With the global virtual store, a change to a dependency of one package in a dependency cycle now gives every package in that cycle a new directory. Before, the other packages kept their directories and still linked the old copy of the changed package, so the project could load two copies of it [#17466](https://github.com/pnpm/pnpm/issues/17466).

  Packages in a dependency cycle, and packages that depend on one, move to new global virtual store directories once after this update. Those that run a build miss the side-effects cache once.

- Updated dependencies:
  - @pnpm/lockfile.utils@1102.1.9
