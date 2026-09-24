---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm licenses list` failed or reported nothing in a workspace with `sharedWorkspaceLockfile: false`. It now reads the lockfile of each selected project [#10140](https://github.com/pnpm/pnpm/issues/10140).
