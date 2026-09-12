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
  // Parsed rather than compared to a rebuilt protocol string, so the
  // `catalog:default` spelling of the default catalog counts too.
  return Object.values(importers).some(
    (importer) => parseCatalogProtocol(importer.specifiers[alias] ?? '') === catalogName
  )
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

/**
 * Drop every catalog snapshot entry that no importer references any more,
 * matching what a full resolution records after the same removal.
 */
export function pruneUnreferencedCatalogEntries (lockfile: LockfileObject): void {
  if (lockfile.catalogs == null) return
  for (const [catalogName, catalog] of Object.entries(lockfile.catalogs)) {
    for (const alias of Object.keys(catalog)) {
      if (!catalogEntryIsReferenced(lockfile.importers, catalogName, alias)) {
        delete catalog[alias]
      }
    }
    if (Object.keys(catalog).length === 0) {
      delete lockfile.catalogs[catalogName]
    }
  }
  if (Object.keys(lockfile.catalogs).length === 0) {
    delete lockfile.catalogs
  }
}
