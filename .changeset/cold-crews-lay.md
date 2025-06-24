---
"pnpm": patch
---

Fix a bug in which `pnpm ls --filter=not-exist --json` prints nothing instead of an empty array [#9672](https://github.com/pnpm/pnpm/issues/9672).
