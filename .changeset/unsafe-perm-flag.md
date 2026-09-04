---
"pacquet": patch
---

`pnpm install --unsafe-perm` and the `--unsafe-perm` flag on every other command now work. pnpm 12 rejected the flag with `unexpected argument '--unsafe-perm' found`, which failed every install on Vercel [#14346](https://github.com/pnpm/pnpm/issues/14346).
