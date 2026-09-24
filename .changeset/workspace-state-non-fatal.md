---
"pacquet": patch
---

`pnpm install` no longer fails when writing the workspace state file encounters an error. Failures to update the state file now emit a warning instead of aborting the install [pnpm/pnpm#14550](https://github.com/pnpm/pnpm/issues/14550).
