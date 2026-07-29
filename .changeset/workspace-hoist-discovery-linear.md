---
"pacquet": patch
---

Fixed quadratic time and memory use when resolving large multi-project workspaces from scratch. Peer-hoist discovery no longer snapshots the whole dependency tree and rebuilds the full dependency graph once per project; on a workspace with 331 projects sharing a 5,000-package graph, full resolution dropped from ~65 s and ~3.7 GiB to ~1.4 s and ~1.1 GiB.
