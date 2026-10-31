---
"pacquet": patch
---

With `enableGlobalVirtualStore`, `pnpm install` again runs a dependency's build scripts in a project whose global virtual store slot another project already built, unless the side-effects cache has build output to restore for it. A build that writes only outside its own package, such as a git hook installer, now takes effect in every project [#17526](https://github.com/pnpm/pnpm/issues/17526).
