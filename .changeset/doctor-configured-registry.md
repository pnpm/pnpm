---
"pnpm": patch
---

`pnpm doctor` now checks the configured default registry and sends its credentials. It used to always ping `https://registry.npmjs.org/` [#15618](https://github.com/pnpm/pnpm/issues/15618).
