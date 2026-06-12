---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Fixed `pnpm deploy` creating extra directories inside the deploy target and breaking the project's own `node_modules` when using a relative deploy path [#10981](https://github.com/pnpm/pnpm/issues/10981).
