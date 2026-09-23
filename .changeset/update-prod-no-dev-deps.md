---
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update --prod` no longer installs devDependencies when run in a project installed with `--prod` [#8038](https://github.com/pnpm/pnpm/issues/8038).
