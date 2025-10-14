---
"@pnpm/modules-yaml": patch
"@pnpm/core": patch
"pnpm": patch
---

Detect changes in dependency build settings (onlyBuiltDependencies, onlyBuiltDependenciesFile, neverBuiltDependencies) and automatically recreate node_modules when settings are modified [#9468](https://github.com/pnpm/pnpm/issues/9468).

