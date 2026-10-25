---
"get-pnpm": patch
---

`npx get-pnpm` no longer sends an auth token from npm config to a registry served over plain HTTP, unless the registry is on the local machine.
