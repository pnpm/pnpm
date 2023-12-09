---
"@pnpm/headless": patch
"pnpm": patch
---

Don't report dependencies with optional dependencies as being added on repeat install. This was a bug in reporting [#7384](https://github.com/pnpm/pnpm/issues/7384).
