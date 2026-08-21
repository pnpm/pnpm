---
"pacquet": patch
---

`pnpm install <pkg>` now adds the package, the same as `pnpm add <pkg>` and matching the JavaScript CLI. It previously ended in a usage error: `pnpm i valibot` printed `error: unexpected argument 'valibot' found` instead of saving the dependency [#13886](https://github.com/pnpm/pnpm/issues/13886).
