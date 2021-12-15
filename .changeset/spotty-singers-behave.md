---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

If making an intersection of peer dependency ranges does not succeed, install should not crash [#4134](https://github.com/pnpm/pnpm/issues/4134).
