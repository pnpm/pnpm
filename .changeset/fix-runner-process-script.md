---
"@pnpm/exec.pnpm-cli-runner": patch
"pnpm": patch
---

`pnpm runtime set` and other sub-commands that spawn pnpm CLI subprocesses now re-use the current running script entry point when executed via Node.js, ensuring the invoked pnpm version is preserved [#14829](https://github.com/pnpm/pnpm/issues/14829).
