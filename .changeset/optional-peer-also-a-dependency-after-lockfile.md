---
"pacquet": patch
---

When an existing lockfile records a package's optional peer as a regular dependency, and a newly added dependency now provides that peer, pnpm records it as a peer [#16901](https://github.com/pnpm/pnpm/issues/16901). The lockfile is the same as installing everything at once.
