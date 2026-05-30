---
"@pnpm/exec.commands": patch
"pnpm": patch
---

`verifyDepsBeforeRun` now scopes the auto-triggered install to the filtered packages when running with `--filter` [#11865](https://github.com/pnpm/pnpm/issues/11865).
