---
"pacquet": patch
---

`pnpm install --no-optional` now installs the peer dependencies a project declares when `autoInstallPeers` is on. The lockfile marked such a peer `optional: true` when another dependency had it as an optional peer.
