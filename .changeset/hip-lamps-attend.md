---
"@pnpm/exportable-manifest": patch
"@pnpm/prepare": patch
"pnpm": patch
---

Fix version number replacing for namespaced workspace packages. `workspace:@foo/bar@*` should be replaced with `npm:@foo/bar@<version>` on publish [#6052](https://github.com/pnpm/pnpm/pull/6052).
