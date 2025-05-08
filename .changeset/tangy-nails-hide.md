---
"@pnpm/plugin-commands-installation": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/workspace.manifest-writer": minor
"@pnpm/core": minor
"@pnpm/config": minor
"pnpm": minor
---

Add a CLI flag named `--save-catalog` to `pnpm add` to save new dependencies as a catalog: `catalog:` will be added to `package.json` and the package specifier will be added to the default catalog object (either `catalog` or `catalogs.default`) in `pnpm-workspace.yaml` [#9425](https://github.com/pnpm/pnpm/issues/9425).
