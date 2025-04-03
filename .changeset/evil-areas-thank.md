---
"@pnpm/store-controller-types": major
"@pnpm/resolver-base": major
"@pnpm/npm-resolver": major
"@pnpm/plugin-commands-publishing": patch
"@pnpm/resolve-dependencies": patch
"@pnpm/package-store": major
"@pnpm/package-requester": major
"@pnpm/plugin-commands-store": patch
"@pnpm/outdated": patch
"@pnpm/server": major
"@pnpm/store-connection-manager": major
"@pnpm/core": major
"@pnpm/headless": major
---

The resolving function now takes a `registries` object, so it finds the required registry itself instead of receiving it from package requester.
