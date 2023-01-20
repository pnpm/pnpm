---
"@pnpm/plugin-commands-installation": minor
"@pnpm/core": minor
pnpm: minor
---

Add a `pnpm dedupe` command that removes dependencies from the lockfile by re-resolving the dependency graph. This work similar to yarn's [`yarn dedupe --strategy highest`](https://yarnpkg.com/cli/dedupe) command.
