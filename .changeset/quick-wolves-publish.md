---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": minor
---

Added batch workspace publishing to the Rust CLI. Batch publishing now accepts a scope-specific credential when it applies to every package in the request, and fails before publishing when packages targeting one registry resolve to different credentials [pnpm/pnpm#14101](https://github.com/pnpm/pnpm/issues/14101).
