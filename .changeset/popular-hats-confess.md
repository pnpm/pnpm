---
"@pnpm/plugin-commands-patching": patch
"pnpm": patch
---

Fix a bug in which `pnpm patch` is unable to bring back old patch without specifying `@version` suffix [#8919](https://github.com/pnpm/pnpm/issues/8919).
