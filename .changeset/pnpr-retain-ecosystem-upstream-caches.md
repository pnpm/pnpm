---
"@pnpm/pnpr": patch
---

`pnpr cache-gc`, server startup, and background cache eviction now retain cached Cargo, Python, and OCI upstream files while their upstream registries remain configured. Before, orphan pruning deleted cached crate archives, Python wheels, source distributions, PEP 658 metadata, and OCI manifests and blobs even when their upstream registry was active.
