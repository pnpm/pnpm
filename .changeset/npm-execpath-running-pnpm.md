---
"@pnpm/cli.meta": patch
"@pnpm/exec.npm-lifecycle": patch
"@pnpm/exec.pnpm-cli-runner": patch
"pnpm": patch
"pacquet": patch
---

Scripts that `pnpx` and `pnx` run now get pnpm itself as `npm_execpath`. A script that ran `$npm_execpath install` there ran `pnpm dlx install`.
