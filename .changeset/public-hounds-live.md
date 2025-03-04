---
"@pnpm/plugin-commands-installation": minor
"@pnpm/plugin-commands-patching": minor
"@pnpm/config": minor
"pnpm": minor
---

`pnpm-workspace.yaml` can now hold all the settings that `.npmrc` accepts. The settings should use camelCase [#9211](https://github.com/pnpm/pnpm/pull/9211).

`pnpm-workspace.yaml` example:

```yaml
verifyDepsBeforeRun: install
optimisticRepeatInstall: true
publicHoistPattern:
- "*types*"
- "!@types/react"
```
