## 1101.2.4

### Patch Changes

- `pnpm add <dir>` now warns when the added directory declares peer dependencies, as `pnpm link` does. The directory is saved as a `link:` dependency, and its peers are not resolved from the project that adds it. Use the `file:` protocol to have them resolved [#5523](https://github.com/pnpm/pnpm/issues/5523).

- `injectWorkspacePackages` now hard links a workspace dependency declared with a relative path, such as `workspace:../foo`, the same way it already does for `workspace:*` [#10446](https://github.com/pnpm/pnpm/issues/10446).

- `pnpm install` no longer fails when a package from the registry declares a `file:` dependency on a directory inside itself, such as `"@types/css-tree": "file:./typings/css-tree"`. pnpm links that dependency to the directory inside the package, as npm and Yarn do. The lockfile records it as `link:<root>/typings/css-tree` [#9141](https://github.com/pnpm/pnpm/issues/9141).

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.7
  - @pnpm/deps.path@1101.0.5
  - @pnpm/error@1100.2.1
  - @pnpm/resolving.resolver-base@1101.3.2
  - @pnpm/workspace.project-manifest-reader@1100.1.1
