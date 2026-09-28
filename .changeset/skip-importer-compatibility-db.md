---
"@pnpm/hooks.read-package-hook": patch
"@pnpm/installing.context": patch
"pnpm": patch
"pacquet": patch
---

pnpm's built-in package compatibility database no longer applies to a project's own manifest. A project named like a published package, such as `vue-loader`, no longer gains dependencies on `pnpm install` or `pnpm update`. User-configured `packageExtensions` still apply to project manifests [#11700](https://github.com/pnpm/pnpm/issues/11700).
