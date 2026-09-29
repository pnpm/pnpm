---
"pacquet": patch
---

Sped up dependency resolution in large workspaces. The resolver now shares one copy of each package id between every node, ancestor chain and cache entry that refers to it, and renders a package's name and version once for all of its peer dependency checks.
