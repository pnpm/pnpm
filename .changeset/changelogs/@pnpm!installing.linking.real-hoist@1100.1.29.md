## 1100.1.29

### Patch Changes

- Fixed `nodeLinker: hoisted` installs omitting a dependency that its parent also declares as a peer dependency when `autoInstallPeers` is disabled [#18193](https://github.com/pnpm/pnpm/issues/18193).

- Fixed `nodeLinker: hoisted` installs in a workspace where the directory of one project is inside the directory of another. A dependency of the inner project could resolve to the version in the outer project's `node_modules`. pnpm now keeps the inner project's dependency in its own `node_modules` when the outer project holds another version.

- Updated dependencies:
  - @pnpm/deps.path@1101.0.11
  - @pnpm/error@1100.2.3
  - @pnpm/lockfile.types@1100.1.9
  - @pnpm/lockfile.utils@1102.1.13
