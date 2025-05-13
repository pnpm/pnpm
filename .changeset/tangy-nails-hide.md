---
"@pnpm/plugin-commands-installation": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/workspace.read-manifest": minor
"@pnpm/workspace.manifest-writer": minor
"@pnpm/core": minor
"@pnpm/config": minor
"pnpm": minor
---

Add a CLI option (`--save-catalog=<name>`) to `pnpm add` to save new dependencies as a catalog: `catalog:<name>` or `catalog:` will be added to `package.json` and the package specifier will be added to the `catalogs` or `catalog` object in `pnpm-workspace.yaml` [#9425](https://github.com/pnpm/pnpm/issues/9425).
