import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import * as dp from '@pnpm/deps.path'
import type { CatalogSnapshots, ProjectSnapshot } from '@pnpm/lockfile.types'
import semver from 'semver'

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
  if (ref == null) return false
  const resolvedVersion = dp.removeSuffix(ref)
  if (semver.valid(resolvedVersion) == null) return false
  return resolvedVersion !== catalogVersion
}
