---
"pacquet": patch
---

pnpm rejects a `registries` entry whose name matches a reserved specifier prefix in any case, such as `PKG` or `Npm`. A selector like `pnpm add PKG:foo` reads the prefix rather than the registry, so such an entry could not be used.
