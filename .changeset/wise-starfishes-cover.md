---
"@pnpm/config": minor
"pnpm": minor
---

A new settig has been added called `dedupe-direct-deps`, which is disabled by default. When set to `true`, dependencies that are already symlinked to the root `node_modules` directory of the workspace will not be symlinked to subproject `node_modules` directories. This feature was enabled by default in v8.0.0 but caused issues, so it's best to disable it by default [#6299](https://github.com/pnpm/pnpm/issues/6299).
