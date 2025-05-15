---
"@pnpm/plugin-commands-installation": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/workspace.read-manifest": minor
"@pnpm/workspace.manifest-writer": minor
"@pnpm/core": minor
"@pnpm/config": minor
"pnpm": minor
---

Added two new CLI options (`--save-catalog` and `--save-catalog-name=<name>`) to `pnpm add` to save new dependencies as catalog entries. `catalog:` or `catalog:<name>` will be added to `package.json` and the package specifier will be added to the `catalogs` or `catalog[<name>]` object in `pnpm-workspace.yaml` [#9425](https://github.com/pnpm/pnpm/issues/9425).
