---
"@pnpm/deps.inspection.commands": minor
"@pnpm/workspace.projects-filter": patch
"@pnpm/workspace.root-finder": patch
"pnpm": minor
---

feat(view): support searching project manifest upward when package name is omitted

When running `pnpm view` without a package name, the command now searches
upward for the nearest project manifest (`package.json`, `package.yaml`, or `package.json5`) and uses its `name` field.
If the manifest exists but lacks a `name` field, an error is thrown.

This change also replaces the `find-up` dependency with `empathic` for
improved performance and consistency across workspace tools.
