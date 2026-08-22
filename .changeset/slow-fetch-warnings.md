---
"pacquet": minor
"@pnpm/napi": minor
---

Added `fetchWarnTimeoutMs` and `fetchMinSpeedKiBps` to the Rust pnpm CLI and its N-API bindings. Slow registry metadata requests and tarball downloads now emit the same warnings as pnpm 11 [pnpm/pnpm#12042](https://github.com/pnpm/pnpm/issues/12042).
