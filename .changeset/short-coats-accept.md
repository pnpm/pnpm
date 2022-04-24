---
"@pnpm/plugin-commands-init": patch
---

`pnpm init` should not fail if one of the parent directories contains a `package.json` file [#4589](https://github.com/pnpm/pnpm/issues/4589).
