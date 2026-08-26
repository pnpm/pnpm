---
"@pnpm/config.normalize-registries": minor
"@pnpm/config.reader": minor
"@pnpm/config.commands": minor
"pnpm": minor
"pacquet": minor
---

`pnpm config get` and `pnpm config list` now show the settings pnpm acts on under their documented names:

- `registries` shows the registries pnpm resolves from, merged across every source (`.npmrc`, `pnpm-workspace.yaml`, the global config, CLI flags), in the shape the setting is written in: keyed by registry URL, with the default registry declared as the bare `@` scope. Built-in routes are included — the `@jsr` scope and the `npmjs` and `gh` prefixes — unless pointed elsewhere. Previously `pnpm config get registries` printed `undefined`.
- `update` and `audit` show the effective sections, whichever spelling set them. The deprecated internal spellings (`updateConfig`, `auditConfig`, `auditLevel`) are no longer listed.
- `catalogs` shows the complete resolved catalog set — the singular `catalog` block is its `default` entry — whichever spelling declared it.
- The `registry` and `@scope:registry` entries show the merged routes rather than raw `.npmrc` values, so they always agree with the `registries` view.
