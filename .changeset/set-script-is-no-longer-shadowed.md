---
"pnpm": patch
---

`pnpm set-script` no longer fails with `ERR_PNPM_NOT_IMPLEMENTED`. The command was implemented but a leftover not-implemented stub was registered under the same name and shadowed it [`pnpm/pnpm#13956`](https://github.com/pnpm/pnpm/issues/13956).
