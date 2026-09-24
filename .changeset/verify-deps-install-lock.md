---
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---
Concurrent `pnpm run` and `pnpm exec` commands now serialize their dependency installs [#14551](https://github.com/pnpm/pnpm/issues/14551).
