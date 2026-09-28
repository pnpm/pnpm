## 1101.1.1

### Patch Changes

- `pnpm install --dev` and `pnpm fetch --dev` now install the optional dependencies of devDependencies, such as the platform binaries of Biome and oxlint. The project's own `optionalDependencies` are still skipped [#9678](https://github.com/pnpm/pnpm/issues/9678).

- The global virtual store and the side-effects cache now key built packages by the Node.js version that the root project's `devEngines.runtime` or `engines.runtime` pins. That is the Node.js their build scripts run with. A dependency that declares its own `engines.runtime` no longer changes the key for every other package.

- `pnpm install` no longer fails when a package from the registry declares a `file:` dependency on a directory inside itself, such as `"@types/css-tree": "file:./typings/css-tree"`. pnpm links that dependency to the directory inside the package, as npm and Yarn do. The lockfile records it as `link:<root>/typings/css-tree` [#9141](https://github.com/pnpm/pnpm/issues/9141).

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.2.1
  - @pnpm/deps.graph-hasher@1100.3.5
  - @pnpm/deps.path@1101.0.5
  - @pnpm/fs.symlink-dependency@1100.0.22
  - @pnpm/hooks.types@1101.0.5
  - @pnpm/installing.modules-yaml@1101.0.5
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/patching.config@1100.1.8
  - @pnpm/store.controller-types@1101.3.2
