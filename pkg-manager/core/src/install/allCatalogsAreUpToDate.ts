import { type CatalogSnapshots } from '@pnpm/lockfile-file'
import { type Catalogs } from '@pnpm/catalogs.types'

export function allCatalogsAreUpToDate (
  catalogsConfig: Catalogs,
  snapshot: CatalogSnapshots | undefined
): boolean {
  return Object.entries(snapshot ?? {})
    .every(([catalogName, catalog]) => Object.entries(catalog ?? {})
      .every(([alias, entry]) => entry.specifier === catalogsConfig[catalogName]?.[alias]))
}
