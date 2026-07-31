---
"pacquet": patch
---

Reduced the peer-resolution walker's memory usage on large dependency graphs. Children realized for a visit that is then served from the `peersCache` are removed from the tree again instead of being retained (previously ~4M never-walked tree nodes on a ~3.5k-package graph), and all children of a node now share one ancestor-chain `Arc` instead of each child cloning the full chain (previously 6.5M clones / ~20 GB of cumulative allocation on the same graph).
