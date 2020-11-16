---
"@pnpm/config": minor
"@pnpm/npm-resolver": minor
"@pnpm/package-requester": minor
"@pnpm/plugin-commands-installation": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/resolver-base": minor
"@pnpm/store-controller-types": minor
"supi": patch
---

New option added: preferWorkspacePackages. When it is `true`, dependencies are linked from the workspace even, when there are newer version available in the registry.
