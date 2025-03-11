---
"@pnpm/resolver-base": minor
"@pnpm/npm-resolver": minor
"@pnpm/resolve-dependencies": patch
"@pnpm/package-requester": patch
"@pnpm/store-controller-types": patch
"@pnpm/core": patch
---

The `@pnpm/npm-resolver` package now returns an `isWorkspacePackage` field with its result. This field will be `true` if the resolved package was requested through the `workspace:` protocol, or if the wanted dependency's name and specifier match a package in the workspace.
