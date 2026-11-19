---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

Fixed `dedupePeers` linking a different copy of a peer than the one used by its project when the same peer version has multiple instances [pnpm/pnpm#18242](https://github.com/pnpm/pnpm/issues/18242).
