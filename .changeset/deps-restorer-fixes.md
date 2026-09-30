---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

`pnpm install` no longer crashes when a `file:` dependency points to a directory named `constructor`.
