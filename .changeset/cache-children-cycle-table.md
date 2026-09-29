---
"pacquet": patch
---

Sped up dependency resolution of workspaces with many peer dependencies. The peer-hoisting rounds no longer recompute the dependency graph's cycle table for every project.
