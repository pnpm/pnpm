---
"pacquet": patch
---

With `nodeLinker: hoisted` and `autoInstallPeers: false`, a package no longer loads a copy of its dependency that was resolved with different peer versions. This could happen when one package version had several copies with different peers in the dependency graph.
