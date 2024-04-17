---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Fixed an issue where optional dependencies were not linked into the dependent's node_modules [#7943](https://github.com/pnpm/pnpm/issues/7943).
