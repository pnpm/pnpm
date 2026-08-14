---
"@pnpm/napi": minor
---

`readConfig` now returns `explicitSettings` — the camelCase names of settings the config cascade set explicitly — so hosts that layer the resolved config over their own defaults can forward only the values the user actually configured.
