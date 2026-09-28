## 1102.2.1

### Patch Changes

- `pnpm install` no longer fails for an injected workspace dependency whose package publishes from a `publishConfig.directory` that its own `prepare` script builds. The injected copy now picks up that directory once `prepare` finishes building it. `pnpm install --frozen-lockfile` no longer reports the dependency as outdated while the directory has not been built yet. [pnpm/pnpm#7811](https://github.com/pnpm/pnpm/issues/7811)

- `pnpm install` no longer fails when a package from the registry declares a `file:` dependency on a directory inside itself, such as `"@types/css-tree": "file:./typings/css-tree"`. pnpm links that dependency to the directory inside the package, as npm and Yarn do. The lockfile records it as `link:<root>/typings/css-tree` [#9141](https://github.com/pnpm/pnpm/issues/9141).

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.2.1
  - @pnpm/deps.path@1101.0.5
  - @pnpm/error@1100.2.1
  - @pnpm/exec.prepare-package@1100.0.39
  - @pnpm/fetching.fetcher-base@1100.2.12
  - @pnpm/fetching.pick-fetcher@1100.1.13
  - @pnpm/fs.graceful-fs@1100.2.4
  - @pnpm/hooks.types@1101.0.5
  - @pnpm/resolving.local-resolver@1101.2.4
  - @pnpm/resolving.resolver-base@1101.3.2
  - @pnpm/store.cafs@1100.3.5
  - @pnpm/store.controller-types@1101.3.2
  - @pnpm/store.index@1100.3.3
