---
"@pnpm/resolve-dependencies": patch
---

When a new peer dependency is installed, don't remove the existing regular dependencies of the package that depends on the peer.
