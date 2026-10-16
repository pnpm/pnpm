---
"pnpm": patch
"pacquet": patch
---

`pnpm pkg fix` now normalizes a shortcut `repository` definition into the full object format, so `github:pnpm/example` becomes `{ "type": "git", "url": "git+https://github.com/pnpm/example.git" }`. This matches `npm pkg fix` [#16980](https://github.com/pnpm/pnpm/issues/16980).
