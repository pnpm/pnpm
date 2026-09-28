## 1100.3.5

### Patch Changes

- The global virtual store and the side-effects cache now key built packages by the Node.js version that the root project's `devEngines.runtime` or `engines.runtime` pins. That is the Node.js their build scripts run with. A dependency that declares its own `engines.runtime` no longer changes the key for every other package.

- `pnpm install` now restores cached build artifacts when reinstalling a workspace that uses separate lockfiles [#12942](https://github.com/pnpm/pnpm/issues/12942).

- `pnpm install` no longer fails when a package from the registry declares a `file:` dependency on a directory inside itself, such as `"@types/css-tree": "file:./typings/css-tree"`. pnpm links that dependency to the directory inside the package, as npm and Yarn do. The lockfile records it as `link:<root>/typings/css-tree` [#9141](https://github.com/pnpm/pnpm/issues/9141).

- The side-effects cache now restores the symlinks that a build script creates inside a package. A warm install used to replace each of them with a copy of its target [#12859](https://github.com/pnpm/pnpm/issues/12859).

  After upgrading, every package with a build script is built once more.

- Updated dependencies:
  - @pnpm/deps.path@1101.0.5
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/resolving.resolver-base@1101.3.2
