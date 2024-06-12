---
"@pnpm/plugin-commands-installation": patch
"@pnpm/plugin-commands-script-runners": patch
"@pnpm/read-projects-context": patch
"@pnpm/plugin-commands-rebuild": patch
"@pnpm/get-context": patch
"@pnpm/fs.find-packages": patch
"@pnpm/core": patch
"@pnpm/types": patch
"pnpm": patch
---

If install is performed on a subset of workspace projects, always create an up-to-date lockfile first. So, a partial install can be performed only on a fully resolved (non-partial) lockfile [#8165](https://github.com/pnpm/pnpm/issues/8165).
