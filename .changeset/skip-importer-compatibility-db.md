---
"@pnpm/hooks.read-package-hook": patch
"@pnpm/installing.context": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update` no longer adds dependencies when a project's name and version match an entry in the built-in package compatibility database [#11700](https://github.com/pnpm/pnpm/issues/11700). User-configured package extensions continue to apply to project manifests.
