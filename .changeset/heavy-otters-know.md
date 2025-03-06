---
"@pnpm/plugin-commands-deploy": patch
"pnpm": patch
---

`pnpm deploy` should not remove fields from the deployed package's `package.json` file [#9215](https://github.com/pnpm/pnpm/issues/9215).
