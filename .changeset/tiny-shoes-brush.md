---
"@pnpm/constants": patch
"pnpm": patch
---

The full metadata cache should be stored not at the same location as the abbreviated metadata. This fixes a bug where pnpm was loading the abbreviated metadata from cache and couldn't find the "time" field as a result [#9963](https://github.com/pnpm/pnpm/issues/9963).
