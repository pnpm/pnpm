---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --prod` and other installs that skip `devDependencies` no longer run the `pnpm:devPreinstall` script [#7065](https://github.com/pnpm/pnpm/issues/7065).
