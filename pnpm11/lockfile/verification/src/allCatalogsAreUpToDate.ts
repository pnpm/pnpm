import type { Catalogs } from '@pnpm/catalogs.types'
import type { CatalogSnapshots } from '@pnpm/lockfile.types'

import { dependencySpecifiersAreEqual } from './gitSpecifiersAreEquivalent.js'

export function allCatalogsAreUpToDate (
  catalogsConfig: Catalogs,
  snapshot: CatalogSnapshots | undefined
): boolean {
  return Object.entries(snapshot ?? {})
    .every(([catalogName, catalog]) => Object.entries(catalog ?? {})
      .every(([alias, entry]) => dependencySpecifiersAreEqual(entry.specifier, catalogsConfig[catalogName]?.[alias])))
}
