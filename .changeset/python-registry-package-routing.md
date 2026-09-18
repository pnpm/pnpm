---
"pacquet": patch
---

Python `registries` entries now route packages by exact names or trailing-prefix patterns in `packages`. Registry declaration order no longer affects resolution. A matched package resolves exclusively from its assigned registry, including transitive and build dependencies. Use `packages: ["*"]` to declare the default index.
