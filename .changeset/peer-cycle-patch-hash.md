---
"@pnpm/lockfile.fs": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` no longer re-resolves an up-to-date lockfile on every run when a patched package is a peer in a peer cycle [#16418](https://github.com/pnpm/pnpm/issues/16418).
