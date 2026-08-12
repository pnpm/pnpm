---
"pacquet": patch
---

Reduced peak memory usage while resolving peer dependencies. Workspaces with large, deeply peer-dependent dependency graphs could need gigabytes to install; the same install now needs meaningfully less.
