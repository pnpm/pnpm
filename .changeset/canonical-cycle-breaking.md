---
"pacquet": minor
---

**Breaking change.** Dependency cycles are now broken canonically during peer resolution: the members of each cycle are ordered by package id, and the edges that close a cycle are always cut at the same place, no matter where the installation walks into the cycle from. Previously the cut depended on the walk path, so installing the same dependencies could produce different lockfiles depending on importer order or resolution order [#13846](https://github.com/pnpm/pnpm/issues/13846), and a peer-resolution verdict computed for one occurrence of a cyclic package could be wrongly reused at another [#13865](https://github.com/pnpm/pnpm/issues/13865).

With canonical cycle breaking the lockfile is a pure function of the dependency graph: repeated installs, reordered importers, and reordered dependencies all produce byte-identical lockfiles. Peer dependencies of packages inside a cycle keep nearest-wins resolution along the canonical order, and a dependency edge that closes a cycle references an occurrence of its target resolved at the importer level. On large cycle-heavy workspaces peer resolution is 2–3× faster, uses about 25% less memory, and produces a substantially smaller lockfile (fewer redundant peer variants).

Existing lockfiles keep working: headless (`--frozen-lockfile`) installs consume them unchanged, and installs that skip resolution leave them untouched. The first install that actually re-resolves (for example after a dependency change) re-keys walk-order-dependent peer variants of cyclic packages once.
