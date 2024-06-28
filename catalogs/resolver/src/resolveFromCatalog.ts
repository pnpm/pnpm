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

  // Ban catalog entries that use the workspace protocol for a few reasons:
  //
  //   1. It's kind of silly. It'd be better to encourage users to use the
  //      workspace protocol directly.
  //   2. Catalogs cache the resolved version of a dependency specifier in
  //      pnpm-lock.yaml for more consistent resolution across importers. The
  //      link: resolutions can't be shared between importers.
  const protocolOfLookup = catalogLookup.split(':')[0]
  if (protocolOfLookup === 'workspace') {
    return {
      type: 'misconfiguration',
      catalogName,
      error: new PnpmError(
        'CATALOG_ENTRY_INVALID_WORKSPACE_SPEC',
        `The workspace protocol cannot be used as a catalog value. The entry for '${wantedDependency.alias}' in catalog '${catalogName}' is invalid.`),
    }
  }

  // A future version of pnpm will try to support this. These protocols aren't
  // supported today since these are often relative file paths that users expect
  // to be relative to the repo root rather than the location of the pnpm
  // workspace package.
  if (['link', 'file'].includes(protocolOfLookup)) {
    return {
      type: 'misconfiguration',
      catalogName,
      error: new PnpmError(
        'CATALOG_ENTRY_INVALID_SPEC',
        `The entry for '${wantedDependency.alias}' in catalog '${catalogName}' declares a dependency using the '${protocolOfLookup}' protocol. This is not yet supported, but may be in a future version of pnpm.`),
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
