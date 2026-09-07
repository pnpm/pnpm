---
"pacquet": patch
---

`pnpm install <pkg>` now accepts `--offline` and `--prefer-offline`. These flags already worked with `pnpm add <pkg>`, but the install spelling failed with `unexpected argument` [#14194](https://github.com/pnpm/pnpm/pull/14194).
