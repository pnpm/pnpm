---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pacquet": patch
"pnpm": patch
---

When `enableGlobalVirtualStore` is enabled, `pnpm install` now creates symlinks for hoisted transitive dependencies inside each package's `node_modules` directory in the global virtual store. Packages executing from the links store can now resolve their hoisted transitive dependencies at runtime [#9618](https://github.com/pnpm/pnpm/issues/9618).
