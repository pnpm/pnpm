---
"@pnpm/plugin-commands-server": patch
"@pnpm/server": patch
pnpm: patch
---

Fix a bug causing the pnpm server to hang if a tarball worker was requested while another worker was exiting.
