import { PnpmError } from '@pnpm/error'
import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import { type Catalogs } from '@pnpm/catalogs.types'

export interface WantedDependency {
  readonly pref: string
  readonly alias: string
}

/**
 * Dereferences a wanted dependency using the catalog protocol and returns the
 * configured version.
 *
 * Example: catalog:default -> ^1.2.3
 *
 * @returns The catalog entry or undefined if the wanted dependency does not use
 * the catalog protocol.
 *
 * @throws {@link PnpmError} if the catalog entry does not exist.
 */
export type CatalogResolver = (wantedDependency: WantedDependency) => CatalogResolution | undefined

export interface CatalogResolution {
  /**
   * The name of the catalog the resolved specifier was defined in.
   */
  readonly catalogName: string

  /**
   * The specifier that should be used for the wanted dependency.
   */
  readonly entrySpecifier: string
}

export function getCatalogResolver (catalogs: Catalogs): CatalogResolver {
  return function resolveFromCatalog (wantedDependency: WantedDependency): CatalogResolution | undefined {
    const catalogName = parseCatalogProtocol(wantedDependency.pref)

    if (catalogName == null) {
      return undefined
    }

    const catalogLookup = catalogs[catalogName]?.[wantedDependency.alias]
    if (catalogLookup == null) {
      throw new PnpmError(
        'CATALOG_ENTRY_NOT_FOUND_FOR_SPEC',
        `No catalog entry was found for catalog ${catalogName} and ${wantedDependency.alias}.`)
    }

    return {
      catalogName,
      entrySpecifier: catalogLookup,
    }
  }
}
