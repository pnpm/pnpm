---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

Handle Ctrl+C gracefully during the `verifyDepsBeforeRun: prompt` confirmation dialog.

Previously, pressing Ctrl+C during the prompt would crash pnpm with an `ERR_USE_AFTER_CLOSE` error and show a stack trace. Now it exits cleanly with exit code 1.

Fixes #10888
