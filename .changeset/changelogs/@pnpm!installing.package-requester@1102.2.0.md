## 1102.2.0

### Minor Changes

- Added the `forceIgnoresPlatform` setting. When it is `false`, `pnpm install --force` skips optional dependencies whose `os`, `cpu` or `libc` do not match the host instead of installing all of them. The default stays `true` [#6133](https://github.com/pnpm/pnpm/issues/6133).

### Patch Changes

- Installing or adding dependencies no longer fails when a previously installed local tarball file was deleted from disk [#8367](https://github.com/pnpm/pnpm/issues/8367).

- `pnpm install` and `pnpm add` no longer skip optional dependencies that the Node.js version resolved for a `devEngines.runtime` range supports, when the range uses `onFail: download`. An explicitly set `nodeVersion` still takes priority [#14628](https://github.com/pnpm/pnpm/issues/14628).

- `pnpm install --engine-strict` now respects `engines` relaxed by `readPackage` hooks in `.pnpmfile.cjs` [pnpm/pnpm#15482](https://github.com/pnpm/pnpm/issues/15482).

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.2.0
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/deps.path@1101.0.4
  - @pnpm/error@1100.2.0
  - @pnpm/exec.prepare-package@1100.0.38
  - @pnpm/fetching.fetcher-base@1100.2.11
  - @pnpm/fetching.pick-fetcher@1100.1.12
  - @pnpm/fs.graceful-fs@1100.2.3
  - @pnpm/hooks.types@1101.0.4
  - @pnpm/resolving.resolver-base@1101.3.1
  - @pnpm/store.cafs@1100.3.4
  - @pnpm/store.controller-types@1101.3.1
  - @pnpm/store.index@1100.3.2
  - @pnpm/types@1102.1.1
