---
"@pnpm/plugin-commands-env": patch
---

pnpm no longer downloads the required `use-node-version` if the running node version is the same as the wanted version

The required `use-node-version` is no longer downloaded if the running Node version is the same as the wanted version [#8673](https://github.com/pnpm/pnpm/pull/8673).
