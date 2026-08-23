## 1102.0.15

### Patch Changes

- Added `fetchWarnTimeoutMs` and `fetchMinSpeedKiBps` to the Rust pnpm CLI and its N-API bindings. Slow registry metadata requests and tarball downloads now emit pnpm-compatible warnings without exposing URL credentials, query parameters, fragments, or control characters [pnpm/pnpm#12042](https://github.com/pnpm/pnpm/issues/12042).

- Updated dependencies:
  - @pnpm/core-loggers@1100.3.3
  - @pnpm/error@1100.1.3
  - @pnpm/exec.prepare-package@1100.0.33
  - @pnpm/fetching.fetcher-base@1100.2.8
  - @pnpm/store.index@1100.2.5
  - @pnpm/types@1102.0.0
