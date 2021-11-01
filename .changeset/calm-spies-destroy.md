---
"@pnpm/fetcher-base": minor
---

The files response can point to files that are not in the global content-addressable store. In this case, the response will contain a `local: true` property, and the structure of `filesIndex` will be just a `Record<string, string>`.
