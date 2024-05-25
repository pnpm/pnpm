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
 */
export type CatalogResolver = (wantedDependency: WantedDependency) => CatalogResolutionResult

export type CatalogResolutionResult = CatalogResolutionFound | CatalogResolutionMisconfiguration | CatalogResolutionUnused

export interface CatalogResolutionFound {
  readonly type: 'found'
  readonly resolution: CatalogResolution
}

export interface CatalogResolution {
  /**
   * The name of the catalog the resolved specifier was defined in.
   */
  readonly catalogName: string

  /**
   * The specifier that should be used for the wanted dependency. This is a
   * usable version that replaces the catalog protocol with the relevant user
   * defined specifier.
   */
  readonly specifier: string
}

/**
 * The user misconfigured a catalog entry. The entry could be missing or
 * invalid.
 */
export interface CatalogResolutionMisconfiguration {
  readonly type: 'misconfiguration'

  /**
   * Convenience error to rethrow.
   */
  readonly error: PnpmError
  readonly catalogName: string
}

/**
 * The wanted dependency does not use the catalog protocol.
 */
export interface CatalogResolutionUnused {
  readonly type: 'unused'
}

export function resolveFromCatalog (catalogs: Catalogs, wantedDependency: WantedDependency): CatalogResolutionResult {
  const catalogName = parseCatalogProtocol(wantedDependency.pref)

  if (catalogName == null) {
    return { type: 'unused' }
  }

  const catalogLookup = catalogs[catalogName]?.[wantedDependency.alias]
  if (catalogLookup == null) {
    return {
      type: 'misconfiguration',
      catalogName,
      error: new PnpmError(
        'CATALOG_ENTRY_NOT_FOUND_FOR_SPEC',
        `No catalog entry '${wantedDependency.alias}' was found for catalog '${catalogName}'.`),
    }
  }

  if (parseCatalogProtocol(catalogLookup) != null) {
    return {
      type: 'misconfiguration',
      catalogName,
      error: new PnpmError(
        'CATALOG_ENTRY_INVALID_RECURSIVE_DEFINITION',
        `Found invalid catalog entry using the catalog protocol recursively. The entry for '${wantedDependency.alias}' in catalog '${catalogName}' is invalid.`),
    }
  }

  return {
    type: 'found',
    resolution: {
      catalogName,
      specifier: catalogLookup,
    },
  }
}
