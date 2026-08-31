---
"pacquet": patch
---

Installs whose lockfile carries platform or engine constraints are up to ~150 ms faster when resolution runs: the `node --version` probe behind the installability checks now starts before the lockfile is parsed and finishes while dependencies resolve, instead of running afterwards.
