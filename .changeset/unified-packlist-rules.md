---
"pacquet": patch
---

`pnpm pack` now supports extended glob patterns in `files` and preserves explicitly listed internal directory symlinks below excluded directories. Entry points honor nested ignore files, and ignore negations can re-include default-excluded logs and backup files. Nested lockfiles follow the package's file-selection rules.
