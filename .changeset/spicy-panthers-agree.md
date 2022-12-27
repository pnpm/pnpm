---
"@pnpm/config": patch
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

Only the `pnpm add --global <pkg>` command should fail if there is no global pnpm bin directory in the system PATH [#5841](https://github.com/pnpm/pnpm/issues/5841).
