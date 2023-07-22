---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Installation succeeds if a non-optional dependency of an optional dependency has failing installation scripts [#6822](https://github.com/pnpm/pnpm/issues/6822).
