---
"@pnpm/exec.pnpm-cli-runner": patch
"pnpm": patch
---

Commands that run pnpm again, such as `pnpm runtime set` and `pnpm env use`, no longer re-run a script that only looks like pnpm. A script named `pnpm` or `pn` that another package installed was run as though it were pnpm.
