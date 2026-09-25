## 1101.3.1

### Patch Changes

- `pnpm install` and `pnpm add` no longer skip optional dependencies that the Node.js version resolved for a `devEngines.runtime` range supports, when the range uses `onFail: download`. An explicitly set `nodeVersion` still takes priority [#14628](https://github.com/pnpm/pnpm/issues/14628).

- `pnpm install --engine-strict` now respects `engines` relaxed by `readPackage` hooks in `.pnpmfile.cjs` [pnpm/pnpm#15482](https://github.com/pnpm/pnpm/issues/15482).

- Updated dependencies:
  - @pnpm/fetching.fetcher-base@1100.2.11
  - @pnpm/resolving.resolver-base@1101.3.1
  - @pnpm/types@1102.1.1
