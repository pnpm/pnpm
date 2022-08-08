---
"@pnpm/package-requester": major
"@pnpm/pick-fetcher": major
"@pnpm/tarball-fetcher": major
"@pnpm/node.fetcher": patch
---

Refactor `tarball-fetcher` and separate it into more specific fetchers, such as `localTarball`, `remoteTarball` and `gitHostedTarball`.
