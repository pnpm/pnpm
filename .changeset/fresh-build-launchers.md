---
"@pnpm/bins.linker": patch
"@pnpm/building.after-install": patch
"pnpm": patch
"pacquet": patch
---

`pnpm rebuild` and `pnpm approve-builds` refresh command launchers when a build changes a command's interpreter or replaces it with a native executable.

Dependent packages' build scripts use the refreshed launchers.
