import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import * as dp from '@pnpm/deps.path'
import type { CatalogSnapshots, ProjectSnapshot } from '@pnpm/lockfile.types'

/**
 * Whether every `catalog:` dependency this project recorded resolves to the version its
 * catalog entry did.
 *
 * A catalog entry resolves to one version for the whole workspace, but only the projects in
 * an install that moves the entry follow it. The rest keep the version they had, and their
 * entry's specifier matches by then, so nothing else reads them as out of date.
 */
export function catalogResolutionsAreUpToDate (
  importer: ProjectSnapshot,
  catalogs: CatalogSnapshots | undefined
): boolean {
  if (importer.specifiers == null) return true
  for (const [alias, specifier] of Object.entries(importer.specifiers)) {
    if (!catalogResolutionIsStale({ importer, catalogs, alias, specifier })) continue
    return false
  }
  return true
}

export function catalogResolutionIsStale (
  { importer, catalogs, alias, specifier }: {
    importer: ProjectSnapshot
    catalogs: CatalogSnapshots | undefined
    alias: string
    specifier: string | undefined
  }
): boolean {
  if (specifier == null) return false
  const catalogName = parseCatalogProtocol(specifier)
  if (catalogName === null) return false
  const catalogVersion = catalogs?.[catalogName]?.[alias]?.version
  if (catalogVersion == null) return false
  const ref = importer.dependencies?.[alias] ?? importer.devDependencies?.[alias] ?? importer.optionalDependencies?.[alias]
  // An alias (`npm:` in the catalog entry) records `<name>@<version>` rather than a bare
  // version, and a non-registry resolution records a protocol. Neither compares to a version.
  if (ref == null || ref.includes('@') || ref.includes(':')) return false
  return dp.removeSuffix(ref) !== catalogVersion
}
