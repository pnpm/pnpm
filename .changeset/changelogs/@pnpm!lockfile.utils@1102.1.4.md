## 1102.1.4

### Patch Changes

- `pnpm add` and `pnpm install` now support installing bzip2 compressed tarballs [https://github.com/pnpm/pnpm/issues/6761](https://github.com/pnpm/pnpm/issues/6761).

- `pnpm install` and `pnpm add` no longer skip optional dependencies that the Node.js version resolved for a `devEngines.runtime` range supports, when the range uses `onFail: download`. An explicitly set `nodeVersion` still takes priority [#14628](https://github.com/pnpm/pnpm/issues/14628).

- `pnpm install` now updates an injected workspace dependency after that package's own dependencies change, when `shared-workspace-lockfile` is `false` [#7209](https://github.com/pnpm/pnpm/issues/7209).

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.2
  - @pnpm/deps.path@1101.0.4
  - @pnpm/error@1100.2.0
  - @pnpm/hooks.types@1101.0.4
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/resolving.resolver-base@1101.3.1
  - @pnpm/resolving.tarball-url@1101.1.2
  - @pnpm/types@1102.1.1
