---
"pacquet": patch
---

The `pnpm` binary no longer includes the `chrono` crate, which RustSec reports as unmaintained in RUSTSEC-2026-0543. Timestamps are read and written as before.
