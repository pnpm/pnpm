---
"@pnpm/global.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update --global` no longer reinstalls a global package when its dependency graph resolves to what is already installed. It reports `Already up to date` instead [pnpm/pnpm#12002](https://github.com/pnpm/pnpm/issues/12002).
