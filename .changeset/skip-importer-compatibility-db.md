---
"@pnpm/hooks.read-package-hook": patch
"@pnpm/installing.context": patch
"pnpm": patch
"pacquet": patch
---

Do not apply the built-in package compatibility database to project manifests, so `pnpm update` no longer adds dependencies when a project's name and version match a registry package [#11700](https://github.com/pnpm/pnpm/issues/11700). User-configured package extensions still apply to projects.
