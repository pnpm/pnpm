---
"pacquet": patch
---

Recursive runs such as `pnpm --parallel "/pattern/"` now execute the scripts the selector matched in the same package concurrently, up to `workspaceConcurrency`. They previously ran one at a time, unlike on pnpm 11 [pnpm/pnpm#14933](https://github.com/pnpm/pnpm/issues/14933).
