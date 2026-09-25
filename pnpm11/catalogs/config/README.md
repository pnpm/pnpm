# @pnpm/catalogs.config

> Create a normalized catalogs config from `pnpm-workspace.yaml` contents.

## Usage

### `getCatalogsFromWorkspaceManifest`

Normalizes `catalog` and `catalogs` definitions from a workspace manifest into a single `Catalogs` object. The `default` catalog may be defined either via top-level `catalog` or `catalogs.default`.

```ts
import { getCatalogsFromWorkspaceManifest } from '@pnpm/catalogs.config'

const catalogs = getCatalogsFromWorkspaceManifest({
  catalog: {
    react: '^19.0.0',
  },
  catalogs: {
    react18: {
      react: '^18.3.1',
    },
  },
})
```

### `mergeCatalogs`

Deep-merges catalog definitions, with later arguments taking precedence over earlier ones at the individual entry level.

```ts
import { mergeCatalogs } from '@pnpm/catalogs.config'

const merged = mergeCatalogs(baseCatalogs, updatedCatalogs)
```
