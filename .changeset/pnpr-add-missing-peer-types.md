---
"@pnpm/pnpr": patch
---

When pnpm sends the `addMissingPeerTypes` setting, pnpr now resolves the matching `@types` packages as optional peers of the packages that peer-depend on a library.
