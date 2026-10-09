---
"pacquet": patch
"@pnpm/napi": patch
---

`enable-modules-dir=false` resolves without fetching and then fetches the resolved packages into the store in one batch. Downloading each package while the graph was still resolving slowed both down on large graphs.
