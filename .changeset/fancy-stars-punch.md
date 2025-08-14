---
"@pnpm/plugin-commands-config": minor
"pnpm": minor
---

`pnpm config get` now accepts property paths (e.g. `pnpm config get catalog.react`, `pnpm config get .catalog.react`, `pnpm config get 'packageExtensions["@babel/parser"].peerDependencies["@babel/types"]'`), and `pnpm config set` now accepts dot-leading or subscripted keys (e.g. `pnpm config set .ignoreScripts true`).
