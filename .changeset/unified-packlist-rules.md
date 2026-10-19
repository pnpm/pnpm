---
"pacquet": patch
---

`pnpm pack` now supports extended glob patterns in `files`. Explicitly listed internal directory symlinks are preserved below excluded directories. Entry points honor nested ignore files. Ignore negations can re-include default-excluded logs and backup files. Nested lockfiles follow the package's file selection rules.
