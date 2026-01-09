---
"@pnpm/package-store": patch
"pnpm": patch
---

Do not add a symlink to the project into the store's project registry if the store is in a subdirectory of the project [#10411](https://github.com/pnpm/pnpm/issues/10411).
