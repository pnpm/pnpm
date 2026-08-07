import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import type { Catalogs } from '@pnpm/catalogs.types'
import type { LockfileObject } from '@pnpm/lockfile.types'
import semver from 'semver'

export function tryFastUpdateCatalogs (
  lockfile: LockfileObject,
  opts: {
    catalogs: Catalogs
    overrides: Record<string, string>
  }
): boolean {
  if (Object.values(opts.overrides).some((specifier) => parseCatalogProtocol(specifier) != null)) {
    return false
  }
  if (!catalogReferencesHaveSnapshots(lockfile, opts.catalogs)) return false

  let changed = false
  const catalogs = Object.fromEntries(
    Object.entries(lockfile.catalogs ?? {}).flatMap(([catalogName, catalog]) => {
      const entries = Object.entries(catalog).flatMap(([alias, entry]) => {
        const specifier = opts.catalogs[catalogName]?.[alias]
        if (specifier == null) {
          if (catalogEntryIsReferenced(lockfile.importers, catalogName, alias)) return [[alias, entry]]
          changed = true
          return []
        }
        if (specifier === entry.specifier) return [[alias, entry]]
        if (
          semver.valid(entry.version) == null ||
          semver.validRange(specifier) == null ||
          !semver.satisfies(entry.version, specifier)
        ) {
          return [[alias, entry]]
        }
        changed = true
        return [[alias, { specifier, version: entry.version }]]
      })
      return entries.length === 0 ? [] : [[catalogName, Object.fromEntries(entries)]]
    })
  )

  if (!changed) return false
  lockfile.catalogs = Object.keys(catalogs).length === 0 ? undefined : catalogs
  return true
}

function catalogEntryIsReferenced (
  importers: LockfileObject['importers'],
  catalogName: string,
  alias: string
): boolean {
  const protocol = catalogName === 'default' ? 'catalog:' : `catalog:${catalogName}`
  return Object.values(importers).some((importer) => importer.specifiers[alias] === protocol)
}

/**
 * Whether every catalog an importer references has an entry in both the
 * configuration and the lockfile. Without one there is nothing to rewrite the
 * reference from, so the resolver has to run.
 */
export function catalogReferencesHaveSnapshots (
  lockfile: LockfileObject,
  catalogs: Catalogs
): boolean {
  return Object.values(lockfile.importers).every((importer) =>
    Object.entries(importer.specifiers).every(([alias, specifier]) => {
      const catalogName = parseCatalogProtocol(specifier)
      return catalogName == null || (
        catalogs[catalogName]?.[alias] != null &&
        lockfile.catalogs?.[catalogName]?.[alias] != null
      )
    })
  )
}
