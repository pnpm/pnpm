---
"pnpm": minor
---

New setting added: `scripts-prepend-node-path`. This setting can be `true`, `false`, or `warn-only`.
When `true`, the path to the `node` executable with which pnpm executed is prepended to the `PATH` of the scripts.
When `warn-only`, pnpm will print a warning if the scripts run with a `node` binary that differs from the `node` binary executing the pnpm CLI.
