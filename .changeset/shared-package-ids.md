---
"pacquet": patch
---

Reduced peak memory usage while resolving peer dependencies further: each occurrence in the dependency tree now shares its package id with the edge it came from instead of owning a copy of it.
