---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm self-update` now updates the project's `engines.pnpm` range along with its pnpm pin, keeping the `^` or `~` operator [pnpm/pnpm#17732](https://github.com/pnpm/pnpm/issues/17732).
