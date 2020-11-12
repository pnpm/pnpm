---
"@pnpm/exportable-manifest": minor
"@pnpm/npm-resolver": minor
"@pnpm/plugin-commands-publishing": minor
---

Support aliases to workspace packages. For instance, `"foo": "workspace:bar@*"` will link bar from the repository but aliased to foo. Before publish, these specs are converted to regular aliased versions.
