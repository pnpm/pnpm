import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import type { Catalogs } from '@pnpm/catalogs.types'
import type { WantedDependency } from '@pnpm/installing.deps-resolver'
import type { CatalogSnapshots, LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.fs'
import { catalogResolutionIsStale } from '@pnpm/lockfile.verification'
import { isEmpty, map as mapValues } from 'ramda'

// If the specifier is new, the old resolution probably does not satisfy it anymore.
// By removing these resolutions we ensure that they are resolved again using the new specs.
export function forgetResolutionsOfPrevWantedDeps (
  wantedDeps: WantedDependency[],
  ctx: {
    importer: ProjectSnapshot
    prevCatalogs: CatalogSnapshots | undefined
    catalogsConfig: Catalogs | undefined
  }
): void {
  const { importer, prevCatalogs, catalogsConfig } = ctx
  if (!importer.specifiers) return
  importer.dependencies = importer.dependencies ?? {}
  importer.devDependencies = importer.devDependencies ?? {}
  importer.optionalDependencies = importer.optionalDependencies ?? {}
  for (const { alias, bareSpecifier } of wantedDeps) {
    if (!alias) continue
    const prevBareSpecifier = importer.specifiers[alias]
    if (
      isWantedDepBareSpecifierSame(prevCatalogs, catalogsConfig, alias, prevBareSpecifier, bareSpecifier) &&
      !catalogResolutionIsStale({ importer, catalogs: prevCatalogs, alias, specifier: prevBareSpecifier })
    ) continue
    if (!importer.dependencies[alias]?.startsWith('link:')) {
      delete importer.dependencies[alias]
    }
    delete importer.devDependencies[alias]
    delete importer.optionalDependencies[alias]
  }
}

export function forgetResolutionsOfAllPrevWantedDeps (wantedLockfile: LockfileObject): void {
  if ((wantedLockfile.importers != null) && !isEmpty(wantedLockfile.importers)) {
    wantedLockfile.importers = mapValues(
      ({ dependencies: _dependencies, devDependencies: _devDependencies, optionalDependencies: _optionalDependencies, ...rest }) => rest,
      wantedLockfile.importers)
  }

  // The resolveDependencies function looks at previous PackageSnapshot
  // dependencies/optionalDependencies blocks and merges them with new resolved
  // deps. Clear the previous PackageSnapshot fields so the newly resolved deps
  // are always used.
  if ((wantedLockfile.packages != null) && !isEmpty(wantedLockfile.packages)) {
    wantedLockfile.packages = mapValues(
      ({ dependencies: _dependencies, optionalDependencies: _optionalDependencies, ...rest }) => rest,
      wantedLockfile.packages)
  }
}

/**
 * Check if a wanted bareSpecifier is the same.
 *
 * It would be different if the user modified a dependency in package.json or a
 * catalog entry in pnpm-workspace.yaml. This is normally a simple check to see
 * if the specifier strings match, but catalogs make this more involved since we
 * also have to check if the catalog config in pnpm-workspace.yaml is the same.
 */
function isWantedDepBareSpecifierSame (
  prevCatalogs: CatalogSnapshots | undefined,
  catalogsConfig: Catalogs | undefined,
  alias: string,
  prevBareSpecifier: string | undefined,
  nextBareSpecifier: string
): boolean {
  if (prevBareSpecifier !== nextBareSpecifier) {
    return false
  }

  // When pnpm catalogs are used, the specifiers can be the same (e.g.
  // "catalog:default"), but the wanted versions for the dependency can be
  // different after resolution if the catalog config was just edited.
  const catalogName = parseCatalogProtocol(prevBareSpecifier)

  // If there's no catalog name, the catalog protocol was not used and we
  // can assume the bareSpecifier is the same since prevBareSpecifier and nextBareSpecifier match.
  if (catalogName === null) {
    return true
  }

  const prevCatalogEntrySpec = prevCatalogs?.[catalogName]?.[alias]?.specifier
  const nextCatalogEntrySpec = catalogsConfig?.[catalogName]?.[alias]

  return prevCatalogEntrySpec === nextCatalogEntrySpec
}
