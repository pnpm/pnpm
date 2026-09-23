---
"@pnpm/cli.default-reporter": patch
"@pnpm/exec.npm-lifecycle": patch
"pacquet": patch
"pnpm": patch
---

A script killed by a signal now fails with an error that names the signal, such as `Command failed with signal SIGKILL.` [#9821](https://github.com/pnpm/pnpm/issues/9821).
