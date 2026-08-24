---
"pacquet": patch
---

`pnpm update <pkg>@<tag>` now saves the version the dist tag resolved to in `package.json`, keeping the range operator the dependency already declared, instead of saving the tag itself. A dependency declared through a `catalog:` reference, a `workspace:` or `npm:` alias, or a path or git specifier keeps its declaration, and one that already tracks a dist tag records the tag asked for [#14092](https://github.com/pnpm/pnpm/issues/14092).
