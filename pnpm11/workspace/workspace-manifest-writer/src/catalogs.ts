import type { Catalogs } from '@pnpm/catalogs.types'
import { parsePkgAndParentSelector } from '@pnpm/config.parse-overrides'
import type { CatalogSnapshots } from '@pnpm/lockfile.types'
import type { Project } from '@pnpm/types'
import type { WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'

type Catalog = Record<string, string>

type CatalogReferences = Map<string, Set<string>>

export function addCatalogs (manifest: Partial<WorkspaceManifest>, newCatalogs: Catalogs): boolean {
  let shouldBeUpdated = false

  for (const catalogName in newCatalogs) {
    const existingCatalog = findCatalog(manifest, catalogName)
    const { catalog, changed } = mergeCatalogEntries(existingCatalog, newCatalogs[catalogName] ?? {})
    shouldBeUpdated = changed || shouldBeUpdated

    if (catalog == null || existingCatalog != null) continue
    attachCatalog(manifest, catalogName, catalog)
  }

  return shouldBeUpdated
}

function findCatalog (manifest: Partial<WorkspaceManifest>, catalogName: string): Catalog | undefined {
  if (catalogName === 'default') {
    return manifest.catalog ?? manifest.catalogs?.default
  }
  return manifest.catalogs != null && Object.hasOwn(manifest.catalogs, catalogName)
    ? manifest.catalogs[catalogName]
    : undefined
}

function mergeCatalogEntries (
  existingCatalog: Catalog | undefined,
  newEntries: Record<string, string | undefined>
): { catalog: Catalog | undefined, changed: boolean } {
  let catalog = existingCatalog
  let changed = false
  for (const [dependencyName, specifier] of Object.entries(newEntries)) {
    if (specifier == null) {
      continue
    }

    catalog ??= {}
    if (catalog[dependencyName] !== specifier) {
      catalog[dependencyName] = specifier
      changed = true
    }
  }
  return { catalog, changed }
}

function attachCatalog (manifest: Partial<WorkspaceManifest>, catalogName: string, catalog: Catalog): void {
  if (catalogName === 'default') {
    manifest.catalog = catalog
  } else {
    manifest.catalogs ??= {}
    manifest.catalogs[catalogName] = catalog
  }
}

export function removePackagesFromWorkspaceCatalog (manifest: Partial<WorkspaceManifest>, packagesJson: Project[], keptCatalogs?: CatalogSnapshots): boolean {
  if (packagesJson.length === 0 || (manifest.catalog == null && manifest.catalogs == null)) {
    return false
  }
  const packageReferences = collectCatalogReferences(packagesJson, manifest.overrides ?? {})
  addKeptCatalogReferences(packageReferences, keptCatalogs ?? {})
  const defaultCatalogChanged = pruneDefaultCatalog(manifest, packageReferences)
  const namedCatalogsChanged = pruneNamedCatalogs(manifest, packageReferences)
  return defaultCatalogChanged || namedCatalogsChanged
}

function collectCatalogReferences (packagesJson: Project[], overrides: Record<string, string>): CatalogReferences {
  const packageReferences: CatalogReferences = new Map()

  for (const pkg of packagesJson) {
    const pkgManifest = pkg.manifest
    const dependencyTypes = [
      pkgManifest.dependencies,
      pkgManifest.devDependencies,
      pkgManifest.optionalDependencies,
      pkgManifest.peerDependencies,
    ]

    for (const deps of dependencyTypes) {
      addPackageReferences(packageReferences, deps ?? {})
    }
  }

  for (const [selector, version] of Object.entries(overrides)) {
    if (!version.startsWith('catalog:')) {
      continue
    }
    const pkgName = overrideTargetName(selector)
    if (pkgName == null) continue
    addPackageReference(packageReferences, pkgName, version)
  }

  return packageReferences
}

function addKeptCatalogReferences (packageReferences: CatalogReferences, keptCatalogs: CatalogSnapshots): void {
  for (const [catalogName, catalog] of Object.entries(keptCatalogs)) {
    const reference = catalogName === 'default' ? 'catalog:' : `catalog:${catalogName}`
    for (const pkgName of Object.keys(catalog)) {
      addPackageReference(packageReferences, pkgName, reference)
    }
  }
}

function addPackageReferences (packageReferences: CatalogReferences, deps: Record<string, string>): void {
  for (const [pkgName, version] of Object.entries(deps)) {
    addPackageReference(packageReferences, pkgName, version)
  }
}

function overrideTargetName (selector: string): string | undefined {
  try {
    return parsePkgAndParentSelector(selector).targetPkg.name
  } catch {
    return undefined
  }
}

function addPackageReference (packageReferences: CatalogReferences, pkgName: string, version: string): void {
  let references = packageReferences.get(pkgName)
  if (references == null) {
    references = new Set()
    packageReferences.set(pkgName, references)
  }
  references.add(version)
}

function pruneDefaultCatalog (manifest: Partial<WorkspaceManifest>, packageReferences: CatalogReferences): boolean {
  if (!manifest.catalog) return false
  let shouldBeUpdated = false
  const packagesToRemove = Object.keys(manifest.catalog).filter(pkg =>
    !packageReferences.get(pkg)?.has('catalog:')
  )

  for (const pkg of packagesToRemove) {
    delete manifest.catalog[pkg]
    shouldBeUpdated = true
  }

  if (Object.keys(manifest.catalog).length === 0) {
    delete manifest.catalog
    shouldBeUpdated = true
  }
  return shouldBeUpdated
}

function pruneNamedCatalogs (manifest: Partial<WorkspaceManifest>, packageReferences: CatalogReferences): boolean {
  if (!manifest.catalogs) return false
  let shouldBeUpdated = false
  const catalogsToRemove: string[] = []

  for (const [catalogName, catalog] of Object.entries(manifest.catalogs)) {
    if (!catalog) continue

    shouldBeUpdated = pruneNamedCatalog(catalogName, catalog, packageReferences) || shouldBeUpdated

    if (Object.keys(catalog).length === 0) {
      catalogsToRemove.push(catalogName)
      shouldBeUpdated = true
    }
  }

  for (const catalogName of catalogsToRemove) {
    delete manifest.catalogs[catalogName]
  }

  if (Object.keys(manifest.catalogs).length === 0) {
    delete manifest.catalogs
    shouldBeUpdated = true
  }
  return shouldBeUpdated
}

function pruneNamedCatalog (catalogName: string, catalog: Catalog, packageReferences: CatalogReferences): boolean {
  const packagesToRemove = Object.keys(catalog).filter(pkg => {
    const references = packageReferences.get(pkg)
    return !references?.has(`catalog:${catalogName}`) && !references?.has('catalog:')
  })

  for (const pkg of packagesToRemove) {
    delete catalog[pkg]
  }
  return packagesToRemove.length > 0
}
