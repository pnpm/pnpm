---
"@pnpm/cli.default-reporter": patch
"@pnpm/exec.npm-lifecycle": patch
"pnpm": patch
---

If a script is killed by a signal that pnpm survives, such as SIGPIPE, the error now names the signal: `Command failed with signal SIGPIPE.` [#9821](https://github.com/pnpm/pnpm/issues/9821).
