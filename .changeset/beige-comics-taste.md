---
"@pnpm/tools.plugin-commands-self-updater": patch
"pnpm": patch
---

`pnpm self-update` should not leave a directory with a broken pnpm installation if the installation fails.
