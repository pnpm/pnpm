---
"@pnpm/package-requester": minor
"@pnpm/worker": minor
"pnpm": minor
---

Increase the network concurrency on machines with many CPU cores. We pick a network concurrency that is not less than 16 and not more than 64 and it is calculated by the number of pnpm workers multiplied by 3 [#10068](https://github.com/pnpm/pnpm/issues/10068).
