---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` no longer include a file that the `files` field excludes by name, such as `!dist/index.js`, when a later entry such as `dist/**` also matches it.
