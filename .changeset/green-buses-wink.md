---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm add` run from a workspace package directory that has no `package.json` yet now suggests creating one with `pnpm init` [#11345](https://github.com/pnpm/pnpm/issues/11345).
