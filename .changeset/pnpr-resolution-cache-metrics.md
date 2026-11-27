---
"@pnpm/pnpr": minor
---

pnpr's Prometheus metrics now report resolution cache hits, misses, and stores. Hits and stores are split by whether the cached resolution is public or private. They also count pnpr's tarball routing decisions, by whether each tarball URL points at the upstream registry, the install gateway, or a package pnpr hosts.
