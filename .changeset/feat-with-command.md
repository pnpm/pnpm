---
"@pnpm/cli.parse-cli-args": minor
"pnpm": minor
---

Add `pnpm with <version|current> <args...>` command. Runs pnpm at a specific version (or the currently active one) for a single invocation, bypassing the project's `packageManager` and `devEngines.packageManager` pins. Uses the same install mechanism as `pnpm self-update`, caching the downloaded pnpm in the global virtual store for reuse.

Examples:

```
pnpm with current install           # ignore the pinned version, use the running pnpm
pnpm with 11.0.0-rc.1 install       # install using pnpm 11.0.0-rc.1
pnpm with next install              # install using the "next" dist-tag
```
