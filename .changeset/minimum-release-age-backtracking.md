---
"@pnpm/resolving.registry.pkg-metadata-filter": minor
"@pnpm/resolving.resolver-base": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/store.controller-types": minor
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.package-requester": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`minimumReleaseAge` now backs off to an older compatible dependency when the newer version's dependencies are too young. Retry blocks respect named registries. Failures include the dependent chain that required the immature package [#11068](https://github.com/pnpm/pnpm/issues/11068).
