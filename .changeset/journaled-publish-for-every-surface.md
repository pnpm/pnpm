---
"@pnpm/pnpr": patch
---

pnpr now stores an uploaded crate or Python distribution and records it in the registry document in one crash-safe step, as it already does for an npm publish. A server that stopped between the two steps left behind a file that nothing pointed at. The next startup now finishes the publish.
