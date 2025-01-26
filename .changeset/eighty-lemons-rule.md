---
"@pnpm/headless": patch
"@pnpm/core": patch
pnpm: patch
---

When running `pnpm install`, the `preprepare` and `postprepare` scripts of the project should be executed [#8989](https://github.com/pnpm/pnpm/pull/8989).
