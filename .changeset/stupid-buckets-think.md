---
"pnpm": patch
---

Fix a bug in which a dependency that is both optional for one package but non-optional for another is omitted when `optional=false` [#8066](https://github.com/pnpm/pnpm/issues/8066).
