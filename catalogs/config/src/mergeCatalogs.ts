import type { Catalog, Catalogs } from '@pnpm/catalogs.types'

/**
 * Deep-merges catalog definitions, later arguments taking precedence over
 * earlier ones at the individual entry level. Use it to fold the catalog
 * changes produced by an install (`updatedCatalogs`) back into the catalogs
 * read at startup, so the result reflects what was actually written to
 * `pnpm-workspace.yaml`.
 *
 * Catalog and dependency names originate from `pnpm-workspace.yaml`, so the
 * result is built from null-prototype records and entries are copied with
 * `Object.defineProperty`. A name like `__proto__` then becomes an ordinary
 * own property instead of mutating a prototype.
 */
export function mergeCatalogs (...catalogsList: Array<Catalogs | undefined>): Catalogs {
  const result = Object.create(null) as Record<string, Catalog>
  for (const catalogs of catalogsList) {
    if (catalogs == null) continue
    for (const catalogName of Object.keys(catalogs)) {
      const catalog = catalogs[catalogName]
      if (catalog == null) continue
      const target: Record<string, string | undefined> = result[catalogName] ?? Object.create(null)
      for (const dependencyName of Object.keys(catalog)) {
        Object.defineProperty(target, dependencyName, {
          value: catalog[dependencyName],
          writable: true,
          enumerable: true,
          configurable: true,
        })
      }
      Object.defineProperty(result, catalogName, {
        value: target,
        writable: true,
        enumerable: true,
        configurable: true,
      })
    }
  }
  return result
}
