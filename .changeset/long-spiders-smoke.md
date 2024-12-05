---
"@pnpm/resolve-dependencies": minor
"@pnpm/package-requester": minor
"@pnpm/store-controller-types": minor
"@pnpm/lockfile.settings-checker": minor
"@pnpm/resolver-base": minor
"@pnpm/npm-resolver": minor
"@pnpm/core": minor
"@pnpm/lockfile.types": minor
"@pnpm/config": minor
"@pnpm/deps.status": minor
"pnpm": minor
---

A new setting, `inject-workspace-packages`, has been added to allow hard-linking all local workspace dependencies instead of symlinking them. Previously, this behavior was achievable via the [`dependenciesMeta[].injected`](https://pnpm.io/package_json#dependenciesmetainjected) setting, which remains supported [#8836](https://github.com/pnpm/pnpm/pull/8836).
