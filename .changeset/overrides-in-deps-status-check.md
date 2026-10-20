---
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

`verifyDepsBeforeRun` and repeat `pnpm install` runs now apply `overrides` before comparing a modified `package.json` with `pnpm-lock.yaml` [#17127](https://github.com/pnpm/pnpm/issues/17127).
