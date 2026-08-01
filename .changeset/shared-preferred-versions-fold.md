---
"pacquet": patch
---

Sped up multi-importer resolution by sharing the run-resolved preferred-versions fold across importers. Every importer replayed the whole workspace's resolved-versions history into a private map each hoist round — O(importers × packages) map inserts and string clones — although the peer-hoist pickers only ever look up a handful of missing-peer names. The fold is now maintained once, workspace-wide, and importers materialize just the buckets they query. Full resolution of a 331-importer benchmark workspace dropped from 886 ms to 424 ms (peer-heavy variant: 2.8 s to 2.4 s).
