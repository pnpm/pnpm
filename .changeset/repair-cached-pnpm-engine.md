---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

`pnpm self-update` now reinstalls a cached engine if its executable cannot run. This repairs shared store entries damaged by an earlier install [pnpm/pnpm#17582](https://github.com/pnpm/pnpm/issues/17582).
