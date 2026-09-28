---
"pacquet": patch
---

Sped up dependency resolution of workspaces with many peer dependencies. Resolving a workspace of 331 projects and 5,000 packages whose packages declare peer dependencies takes 26 to 39 percent less time. The peer-hoisting rounds no longer recompute the dependency graph's cycle table for every project.
