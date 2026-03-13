---
"@pnpm/graph-builder": patch
"pnpm": patch
---

Skip re-importing packages from the global virtual store when `node_modules` is deleted but the store directories are still warm. The global store directory hash already encodes engine, integrity, and full dependency subgraph, so existence is proof of validity.
