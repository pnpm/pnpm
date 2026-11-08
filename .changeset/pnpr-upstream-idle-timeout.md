---
"@pnpm/pnpr": patch
---

A tarball that pnpr downloads from an upstream registry is no longer cut off after the upstream `timeout` while it is still arriving. The `timeout` now limits how long pnpr waits for the upstream to answer and for each further part of the download. Before, a tarball that took longer than 30 seconds to download from a slow upstream failed every time.
