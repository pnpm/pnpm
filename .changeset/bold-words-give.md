---
"@pnpm/plugin-commands-installation": patch
"@pnpm/plugin-commands-deploy": patch
pnpm: patch
---

When the [`enableGlobalVirtualStore`](https://pnpm.io/settings#enableglobalvirtualstore) option is set, the `pnpm deploy` command would incorrectly create symlinks to the global virtual store. To keep the deploy directory self-contained, `pnpm deploy` now ignores this setting and always creates a localized virtual store within the deploy directory.

