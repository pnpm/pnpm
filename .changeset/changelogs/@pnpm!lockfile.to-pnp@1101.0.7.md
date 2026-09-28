## 1101.0.7

### Patch Changes

- `pnpm install` no longer fails when a package from the registry declares a `file:` dependency on a directory inside itself, such as `"@types/css-tree": "file:./typings/css-tree"`. pnpm links that dependency to the directory inside the package, as npm and Yarn do. The lockfile records it as `link:<root>/typings/css-tree` [#9141](https://github.com/pnpm/pnpm/issues/9141).

- With `nodeLinker: pnp`, a workspace package can now require another workspace package it depends on [#3567](https://github.com/pnpm/pnpm/issues/3567). On Windows, workspace dependency paths in the generated `.pnp.cjs` now use forward slashes.

- Updated dependencies:
  - @pnpm/deps.path@1101.0.5
  - @pnpm/fs.symlink-dependency@1100.0.22
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/lockfile.utils@1102.1.5
