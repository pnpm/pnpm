---
"@pnpm/pnpr": minor
---

pnpr's Prometheus metrics now report resolution cache hits, misses, and stores, split by whether the cached resolution is public or private. They also count the tarball URLs pnpr writes into resolve responses, by whether each points at the upstream registry, the install gateway, or a package pnpr hosts.
