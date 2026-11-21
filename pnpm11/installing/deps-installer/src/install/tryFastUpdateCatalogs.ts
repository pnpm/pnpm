import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import type { Catalogs } from '@pnpm/catalogs.types'
import { directoryPeerCatalogReferences } from '@pnpm/installing.deps-resolver'
import type { LockfileObject, PackageSnapshot, ProjectSnapshot, ResolvedCatalogEntry } from '@pnpm/lockfile.types'
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

  const { changed, updatedCatalogs } = rewriteCatalogRanges(lockfile, opts.catalogs)
  if (!changed) return false
  lockfile.catalogs = updatedCatalogs.length === 0 ? undefined : Object.fromEntries(updatedCatalogs)
  return true
}

function rewriteCatalogRanges (
  lockfile: LockfileObject,
  configuredCatalogs: Catalogs
): { changed: boolean, updatedCatalogs: Array<[string, Record<string, ResolvedCatalogEntry>]> } {
  let changed = false
  const updatedCatalogs: Array<[string, Record<string, ResolvedCatalogEntry>]> = []
  for (const [catalogName, catalog] of Object.entries(lockfile.catalogs ?? {})) {
    const entries = Object.entries(catalog).flatMap(([alias, entry]) => {
      const nextEntry = nextCatalogEntry(lockfile, {
        alias,
        catalogName,
        entry,
        specifier: configuredCatalogs[catalogName]?.[alias],
      })
      if (nextEntry !== entry) changed = true
      return nextEntry == null ? [] : [[alias, nextEntry] as const]
    })
    if (entries.length > 0) updatedCatalogs.push([catalogName, Object.fromEntries(entries)])
  }
  return { changed, updatedCatalogs }
}

/**
 * The snapshot a catalog entry gets under the configured `specifier`, the
 * same entry when the range-only rewrite leaves it alone, or `undefined` when
 * it is dropped.
 */
function nextCatalogEntry (
  lockfile: LockfileObject,
  { alias, catalogName, entry, specifier }: {
    alias: string
    catalogName: string
    entry: ResolvedCatalogEntry
    specifier: string | undefined
  }
): ResolvedCatalogEntry | undefined {
  if (specifier == null) {
    return importerReferencesCatalog(lockfile.importers, catalogName, alias) || directoryPeerReferencesCatalog(lockfile, catalogName, alias) ? entry : undefined
  }
  if (specifier === entry.specifier) return entry
  if (directoryPeerReferencesCatalog(lockfile, catalogName, alias)) return entry
  if (!lockedVersionSatisfies(entry.version, specifier)) return entry
  return { specifier, version: entry.version }
}

function lockedVersionSatisfies (version: string, specifier: string): boolean {
  return semver.valid(version) != null &&
    semver.validRange(specifier) != null &&
    semver.satisfies(version, specifier)
}

function importerReferencesCatalog (
  importers: LockfileObject['importers'],
  catalogName: string,
  alias: string
): boolean {
  // Parsed rather than compared to a rebuilt protocol string, so the
  // `catalog:default` spelling of the default catalog counts too.
  return Object.values(importers).some(
    (importer) => parseCatalogProtocol(ownSpecifier(importer, alias)) === catalogName
  )
}

export function directoryPeerReferencesCatalog (lockfile: LockfileObject, catalogName: string, alias: string): boolean {
  for (const [name, dependency] of directoryPeerCatalogReferences(Object.values(lockfile.packages ?? {}) as PackageSnapshot[])) {
    if (catalogName === name && alias === dependency) return true
  }
  return false
}

/**
 * The specifier `importer` records for `alias`, or `''`. Never an inherited
 * `Object.prototype` member, so a dependency named `constructor` is looked up
 * like any other.
 */
export function ownSpecifier (importer: ProjectSnapshot, alias: string): string {
  return Object.hasOwn(importer.specifiers, alias) ? importer.specifiers[alias] ?? '' : ''
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
  const peerReferences = indexDirectoryPeerCatalogReferences(lockfile)
  const isReferenced = (name: string, alias: string): boolean =>
    peerReferences.get(name)?.has(alias) === true || importerReferencesCatalog(lockfile.importers, name, alias)
  for (const [catalogName, catalog] of Object.entries(lockfile.catalogs)) {
    pruneCatalog(catalog, alias => isReferenced(catalogName, alias))
    if (Object.keys(catalog).length === 0) {
      delete lockfile.catalogs[catalogName]
    }
  }
  if (Object.keys(lockfile.catalogs).length === 0) {
    delete lockfile.catalogs
  }
}

function pruneCatalog (catalog: Record<string, ResolvedCatalogEntry>, isReferenced: (alias: string) => boolean): void {
  for (const alias of Object.keys(catalog)) {
    if (!isReferenced(alias)) delete catalog[alias]
  }
}

function indexDirectoryPeerCatalogReferences (lockfile: LockfileObject): Map<string, Set<string>> {
  const peerReferences = new Map<string, Set<string>>()
  for (const [name, alias] of directoryPeerCatalogReferences(Object.values(lockfile.packages ?? {}) as PackageSnapshot[])) {
    if (!peerReferences.has(name)) peerReferences.set(name, new Set())
    peerReferences.get(name)!.add(alias)
  }
  return peerReferences
}
