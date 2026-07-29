---
"pacquet": patch
---

A missing required peer is no longer auto-installed as a prerelease that its declared range rejects. A package peer-depending on `^29.0.0 || ^30.0.0` next to a `30.0.0-alpha.6` pulled in elsewhere in the graph now resolves a stable `29.x`/`30.x` from the registry instead of adopting the alpha [#13341](https://github.com/pnpm/pnpm/issues/13341).
