---
"pacquet": patch
---

Recursive runs now execute the scripts a `/pattern/` selector matched in the same package concurrently. `pnpm --parallel` starts all of them at once. Other recursive runs start up to `workspaceConcurrency` at once. They previously ran one at a time [pnpm/pnpm#14933](https://github.com/pnpm/pnpm/issues/14933).
