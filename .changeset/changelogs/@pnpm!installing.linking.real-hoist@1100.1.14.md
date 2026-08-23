## 1100.1.14

### Patch Changes

- Under `nodeLinker: hoisted`, a dependency declared against a peer-resolution variant of a package version is no longer dropped from the installed layout. All variants of a version share one hoisted copy, and edges pointing at any of them now resolve to it, so the depending project keeps the package in its `.package-map.json` and the depending package keeps it in its `node_modules/.bin`.

- Updated dependencies:
  - @pnpm/deps.path@1101.0.0
  - @pnpm/error@1100.1.3
  - @pnpm/lockfile.types@1100.0.20
  - @pnpm/lockfile.utils@1102.0.0
