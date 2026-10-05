## 1100.0.49

### Patch Changes

- Dependency resolution reads cached registry metadata faster. The metadata cache moved to `<cache-dir>/v12/`, so the first install after upgrading downloads registry metadata again. A damaged cache entry is downloaded again, or reported as an error when `--offline` is set [#13512](https://github.com/pnpm/pnpm/pull/13512).

- Updated dependencies:
  - @pnpm/config.reader@1102.3.4
  - @pnpm/constants@1102.0.1
  - @pnpm/resolving.npm-resolver@1104.2.5
  - @pnpm/store.cafs@1100.3.7
  - @pnpm/store.index@1101.0.1
