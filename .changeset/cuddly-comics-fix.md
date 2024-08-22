---
"@pnpm/tools.plugin-commands-self-updater": major
"@pnpm/tools.path": major
"@pnpm/plugin-commands-installation": minor
"@pnpm/default-reporter": major
"@pnpm/cli-meta": minor
"pnpm": minor
---

Added a new command for upgrading pnpm itself when it isn't managed by Corepack: `pnpm self-update`. This command will work, when pnpm was installed via the standalone script from the [pnpm installation page](https://pnpm.io/installation#using-a-standalone-script) [#8424](https://github.com/pnpm/pnpm/pull/8424).

When executed in a project that has a `packageManager` field in its `package.json` file, pnpm will update its version in the `packageManager` field.
