---
"@pnpm/headless": minor
"@pnpm/hoist": minor
"@pnpm/core": minor
"@pnpm/config": minor
"pnpm": minor
---

A new option added for hoisting packages from the workspace. When `hoist-workspace-packages` is set to `true`, packages from the workspace are symlinked to either `<workspace_root>/node_modules/.pnpm/node_modules` or to `<workspace_root>/node_modules` depending on other hoisting settings (`hoist-pattern` and `public-hoist-pattern`) [#7451](https://github.com/pnpm/pnpm/pull/7451).
