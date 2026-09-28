---
"@pnpm/deps.graph-builder": patch
"@pnpm/deps.graph-hasher": patch
"@pnpm/deps.inspection.tree-builder": patch
"@pnpm/deps.path": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.linking.real-hoist": patch
"@pnpm/installing.package-requester": patch
"@pnpm/lockfile.to-pnp": patch
"@pnpm/releasing.commands": patch
"@pnpm/resolving.local-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` no longer fails when a package from the registry declares a `file:` dependency on a directory inside itself, such as `"@types/css-tree": "file:./typings/css-tree"`. pnpm links that dependency to the directory inside the package, as npm and Yarn do. The lockfile records it as `link:<root>/typings/css-tree` [#9141](https://github.com/pnpm/pnpm/issues/9141).
