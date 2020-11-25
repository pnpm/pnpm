---
"@pnpm/store-connection-manager": patch
---

The maximum number of allowed connections increased to 3 times the number of network concurrency. This should fix the socket timeout issues that sometimes happen.
