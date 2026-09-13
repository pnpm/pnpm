---
"@pnpm/exec.pnpm-cli-runner": patch
"pnpm": patch
---

`pnpm runtime set` and `pnpm env use` now use the pnpm version that started the command. They could run a different installed pnpm when the command was started through Corepack or another wrapper.
