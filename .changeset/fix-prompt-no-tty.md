---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

Handle non-TTY environments correctly when using `verifyDepsBeforeRun: prompt`.

Previously, in non-interactive environments like CI, using `verifyDepsBeforeRun: prompt` would silently exit with code 0 even when node_modules were out of sync. This could cause tests to pass even when they should fail.

Now, pnpm will throw an error in non-TTY environments, alerting users that they need to run `pnpm install` first.

Also handles Ctrl+C gracefully during the prompt - exits cleanly without showing a stack trace.

Fixes #10889, #10888
