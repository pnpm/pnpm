---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm deploy` no longer creates extra directories inside the deploy target and workspace projects when using a relative deploy path [pnpm/pnpm#10981](https://github.com/pnpm/pnpm/issues/10981).
