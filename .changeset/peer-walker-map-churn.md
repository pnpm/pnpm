---
"pacquet": patch
---

Reduced the peer resolution pass's CPU cost on workspaces with many peer dependencies. The walker cloned its parent peer-context maps at every node — twice per node plus once per child — even when a node contributed nothing to them; the maps are now shared copy-on-write and the derived per-child snapshots are reused unless the context actually changed. On a peer-heavy 331-importer benchmark the full resolution dropped from 3.9 s to 2.8 s.
