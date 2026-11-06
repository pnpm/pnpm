---
"pacquet": minor
---

Added `pnpm fund`, which lists the funding URLs declared by the installed dependencies in the same tree `npm fund` prints. Use `--json` for the `npm fund --json` report and `--recursive` or `--filter` to report on workspace projects. `pnpm fund <pkg>` opens the funding URL of an installed package, and `--which` picks one when it lists several [#7354](https://github.com/pnpm/pnpm/issues/7354).
