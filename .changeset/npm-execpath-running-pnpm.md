---
"@pnpm/cli.meta": patch
"@pnpm/exec.npm-lifecycle": patch
"@pnpm/exec.pnpm-cli-runner": patch
"pnpm": patch
---

`npm_execpath` no longer points at `pnpx` in scripts that `pnpx` and `pnx` run. A script that ran `$npm_execpath install` there ran `pnpm dlx install`. `npm_execpath` is now `pnpm` whenever the running pnpm cannot be re-run through its entry script, so such a script runs the pnpm on `PATH`.
