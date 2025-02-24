---
"@pnpm/workspace.injected-deps-syncer": major
"@pnpm/fs.indexed-pkg-importer": minor
"@pnpm/config": minor
"@pnpm/plugin-commands-script-runners": minor
"pnpm": minor
---

Added support for automatically syncing files of injected workspace packages after `pnpm run` [#9081](https://github.com/pnpm/pnpm/issues/9081). Use the `sync-injected-deps-after-scripts` setting to specify which scripts build the workspace package. This tells pnpm when syncing is needed. The setting should be defined in a `.npmrc` file at the root of the workspace. Example:

```ini
sync-injected-deps-after-scripts[]=compile
```
