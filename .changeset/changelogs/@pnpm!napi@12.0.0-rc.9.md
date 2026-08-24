## 12.0.0-rc.9

### Minor Changes

- Added `fetchWarnTimeoutMs` and `fetchMinSpeedKiBps` to the Rust pnpm CLI and its N-API bindings. Slow registry metadata requests and tarball downloads now emit pnpm-compatible warnings without exposing URL credentials, query parameters, fragments, or control characters [pnpm/pnpm#12042](https://github.com/pnpm/pnpm/issues/12042).
