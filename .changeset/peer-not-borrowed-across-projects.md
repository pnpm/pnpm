---
"pacquet": patch
---

With `resolvePeersFromWorkspaceRoot: false`, a workspace project no longer binds a peer to a version that only another project's dependencies provide. pnpm now auto-installs a version that satisfies the peer range for that project. Before, the project got a version outside the range, with no peer warning.
