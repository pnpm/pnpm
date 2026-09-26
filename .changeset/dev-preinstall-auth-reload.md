---
"@pnpm/config.reader": patch
"@pnpm/network.auth-header": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` uses registry auth tokens that `pnpm:devPreinstall` writes to the user `.npmrc` during the same install [#5507](https://github.com/pnpm/pnpm/issues/5507).
