---
"pacquet": patch
---

Fixed `pnpm deploy` failing with `lockfile.format: v13` when an injected workspace package has many dependencies that it also declares as peers.
