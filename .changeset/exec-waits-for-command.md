---
"@pnpm/exec.commands": patch
"@pnpm/exec.npm-lifecycle": patch
"pnpm": patch
---

`pnpm exec` and `pnpm dlx` now wait for the command to finish shutting down after `Ctrl+C`. A signal sent to pnpm alone now reaches the command, the way it does with `pnpm run`. pnpm used to exit on the interrupt and terminate the command while it was still shutting down.
