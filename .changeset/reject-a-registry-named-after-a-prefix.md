---
"pacquet": patch
---

pnpm rejects a `registries` entry named `pkg` in any case, such as `PKG` or `Pkg`. A Package URL's scheme is case-insensitive, so `pnpm add PKG:foo` reads the scheme rather than the registry, and such an entry could not be used.
