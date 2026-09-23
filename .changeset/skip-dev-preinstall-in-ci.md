---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` no longer runs the `pnpm:devPreinstall` script in CI. Setting `ci` to `false` runs it again [#7350](https://github.com/pnpm/pnpm/issues/7350).
