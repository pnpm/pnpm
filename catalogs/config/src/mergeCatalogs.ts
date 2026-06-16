import type { Catalog, Catalogs } from '@pnpm/catalogs.types'

/**
 * Deep-merges catalog definitions, later arguments taking precedence over
 * earlier ones at the individual entry level. Use it to fold the catalog
 * changes produced by an install (`updatedCatalogs`) back into the catalogs
 * read at startup, so the result reflects what was actually written to
 * `pnpm-workspace.yaml`.
 */
export function mergeCatalogs (...catalogsList: Array<Catalogs | undefined>): Catalogs {
  const result: Record<string, Record<string, string | undefined>> = {}
  for (const catalogs of catalogsList) {
    if (catalogs == null) continue
    for (const [catalogName, catalog] of Object.entries(catalogs)) {
      if (catalog == null) continue
      result[catalogName] = { ...result[catalogName], ...(catalog as Catalog) }
    }
  }
  return result
}
