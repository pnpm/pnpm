---
"@pnpm/plugin-commands-installation": minor
"@pnpm/headless": minor
"@pnpm/core": minor
"@pnpm/config": minor
"pnpm": minor
---

Added a new setting called `strict-dep-builds`. When enabled, the installation will exit with a non-zero exit code if any dependencies have unreviewed build scripts (aka postinstall scripts) [#9071](https://github.com/pnpm/pnpm/pull/9071).
