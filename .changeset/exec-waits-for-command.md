---
"@pnpm/exec.commands": patch
"@pnpm/exec.npm-lifecycle": patch
"pnpm": patch
---

`pnpm exec` and `pnpm dlx` now wait for the command to finish shutting down after `Ctrl+C`, and relay a signal sent to pnpm to the command the way `pnpm run` does. pnpm used to exit on the interrupt and terminate the command while it was still shutting down.
