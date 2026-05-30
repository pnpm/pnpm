---
"@pnpm/exec.commands": patch
"pnpm": patch
---

Fixed `verifyDepsBeforeRun` triggering a full workspace install when using `--filter`. The auto-triggered install now respects the filter, so only the filtered package's dependencies are installed.
