---
"pacquet": patch
---

Sped up dependency resolution in large workspaces. Resolving a workspace of 331 projects and 5,000 packages takes half the time when no peer dependencies are involved, and 12 percent less when they are. The resolver now shares one copy of each package id between every node, ancestor chain and cache entry that refers to it.
