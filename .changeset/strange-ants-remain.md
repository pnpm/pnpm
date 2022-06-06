---
"pnpm": patch
"@pnpm/resolve-dependencies": patch
---

When the same package is found several times in the dependency graph, correctly autoinstall its missing peer dependencies at all times [#4820](https://github.com/pnpm/pnpm/issues/4820).
