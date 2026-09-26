---
"@pnpm/config.reader": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

Root `package.json` `resolutions` are now merged with `pnpm-workspace.yaml` `overrides` during workspace installs, with workspace overrides taking precedence [#10675](https://github.com/pnpm/pnpm/issues/10675).
