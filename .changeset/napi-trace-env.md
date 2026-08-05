---
"pacquet": patch
---

The `TRACE` environment variable now enables engine tracing for `@pnpm/napi` consumers the same way it does for the pnpm CLI, and an invalid `TRACE` filter no longer aborts the process — it prints a warning and leaves tracing off.
