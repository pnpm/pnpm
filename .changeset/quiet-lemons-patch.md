---
"pacquet": patch
---

`pnpm install` now keeps patches to bundled dependencies when it restores a package from the side-effects cache, including files a patch adds [#17734](https://github.com/pnpm/pnpm/issues/17734).
