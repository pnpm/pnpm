---
"@pnpm/network.config": patch
"@pnpm/network.agent": patch
"@pnpm/network.fetch": patch
---

`@pnpm/network.config` now exports `isNoProxy`, and `pickSettingByUrl` accepts a `skipFalsyValues` option. `@pnpm/network.agent` and `@pnpm/network.fetch` use them for proxy exclusions and client certificate lookup [pnpm/tasks#59](https://github.com/pnpm/tasks/issues/59).
