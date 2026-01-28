---
"@pnpm/plugin-commands-installation": patch
"@pnpm/plugin-commands-script-runners": patch
"@pnpm/core": patch
"pnpm": patch
---

Fix global build approvals persistence, improve the approve-builds hint for global installs,
and allow `pnpm dlx` to run when `dangerouslyAllowAllBuilds` is enabled.
