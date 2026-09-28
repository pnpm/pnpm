## 1101.0.7

### Patch Changes

- `pnpm install --dev` and `pnpm fetch --dev` now install the optional dependencies of devDependencies, such as the platform binaries of Biome and oxlint. The project's own `optionalDependencies` are still skipped [#9678](https://github.com/pnpm/pnpm/issues/9678).

- `pnpm install` no longer fails when a package from the registry declares a `file:` dependency on a directory inside itself, such as `"@types/css-tree": "file:./typings/css-tree"`. pnpm links that dependency to the directory inside the package, as npm and Yarn do. The lockfile records it as `link:<root>/typings/css-tree` [#9141](https://github.com/pnpm/pnpm/issues/9141).

- `pnpm list` now shows the correct path of a `link:` dependency that points to a directory on another drive on Windows. The path used to be appended to the project directory, such as `C:\project\D:\lib`, and `pnpm list --long` could not show the package's details [#10362](https://github.com/pnpm/pnpm/issues/10362).

- Updated dependencies:
  - @pnpm/deps.path@1101.0.5
  - @pnpm/fs.read-modules-dir@1100.0.3
  - @pnpm/installing.modules-yaml@1101.0.5
  - @pnpm/lockfile.detect-dep-types@1100.0.26
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/lockfile.peer-edges@1100.0.1
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/pkg-manifest.reader@1100.0.21
  - @pnpm/store.cafs@1100.3.5
  - @pnpm/store.index@1100.3.3
