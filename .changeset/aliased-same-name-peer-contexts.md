---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

Fixed aliased dependencies sharing the wrong peer provider across workspace projects when a child uses another version of the aliased package as a peer.
