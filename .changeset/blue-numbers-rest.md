---
"@pnpm/plugin-commands-patching": patch
"pnpm": patch
---

When patching a dependency, only consider files specified in the 'files' field of its package.json. Ignore all others [#6565](https://github.com/pnpm/pnpm/issues/6565)
