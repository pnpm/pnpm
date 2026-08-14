---
"pacquet": patch
---

Peer resolution on large workspaces got faster: each hoist round now refreshes its view of the dependency graph from what the round changed instead of re-reading every resolved package. The resolved dependency graph is unchanged.
