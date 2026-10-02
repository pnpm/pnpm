import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { nameVerFromPkgSnapshot, type PackageSnapshots } from '@pnpm/lockfile.utils'
import { getAllDependenciesFromManifest } from '@pnpm/pkg-manifest.utils'
import {
  DIRECT_DEP_SELECTOR_WEIGHT,
  EXISTING_VERSION_SELECTOR_WEIGHT,
  type PreferredVersions,
  type VersionSelectors,
  type VersionSelectorType,
  type VersionSelectorWithWeight,
} from '@pnpm/resolving.resolver-base'
import type { DependencyManifest, ProjectManifest } from '@pnpm/types'
import getVersionSelectorType from 'version-selector-type'

export function getPreferredVersionsFromLockfileAndManifests (
  snapshots: PackageSnapshots | undefined,
  manifests: Array<DependencyManifest | ProjectManifest>,
  opts: { catalogs?: Catalogs, dedupe?: boolean } = {}
): PreferredVersions {
  const preferredVersions: PreferredVersions = Object.create(null)
  const catalogs = opts.catalogs ?? {}
  for (const manifest of manifests) {
    const specs = getAllDependenciesFromManifest(manifest)
    for (const [name, bareSpecifier] of Object.entries(specs)) {
      addDirectDepSpecifier(preferredVersions, catalogs, name, bareSpecifier)
    }
  }
  if (!snapshots) return preferredVersions
  // Dedupe must let newly resolved dependencies compete with lockfile versions.
  addPreferredVersionsFromLockfile(snapshots, preferredVersions, opts.dedupe ? 1 : EXISTING_VERSION_SELECTOR_WEIGHT)
  return preferredVersions
}

function addDirectDepSpecifier (preferredVersions: PreferredVersions, catalogs: Catalogs, name: string, bareSpecifier: string): void {
  const spec = resolveCatalogSpec(catalogs, name, bareSpecifier)
  if (spec == null) return
  const selector = getVersionSelectorType(spec)
  if (!selector) return
  preferredVersions[name] = preferredVersions[name] ?? (Object.create(null) as VersionSelectors)
  preferredVersions[name][spec] = {
    selectorType: selector.type,
    weight: DIRECT_DEP_SELECTOR_WEIGHT,
  }
}

/**
 * The specifier the resolver installs for a direct dependency: the catalog
 * entry a `catalog:` specifier names, otherwise the specifier itself.
 * `undefined` for a `catalog:` specifier without a usable entry.
 */
function resolveCatalogSpec (catalogs: Catalogs, alias: string, bareSpecifier: string): string | undefined {
  const result = resolveFromCatalog(catalogs, { alias, bareSpecifier })
  switch (result.type) {
    case 'found': return result.resolution.specifier
    case 'misconfiguration': return undefined
    case 'unused': return bareSpecifier
  }
}

function addPreferredVersionsFromLockfile (snapshots: PackageSnapshots, preferredVersions: PreferredVersions, weight: number): void {
  const uniqueNameVersions = collectUniqueNameVersions(snapshots)
  for (const [name, versions] of Object.entries(uniqueNameVersions)) {
    preferredVersions[name] ??= Object.create(null) as VersionSelectors
    const selectors = preferredVersions[name]
    for (const version of versions) {
      addLockfileVersionToSelectors(selectors, name, version, weight)
    }
  }
}

function addLockfileVersionToSelectors (
  selectors: VersionSelectors,
  name: string,
  version: string,
  weight: number
): void {
  const existingSelector = selectors[version]
  if (existingSelector == null) {
    selectors[version] = { selectorType: 'version', weight }
    return
  }

  const existingSelectorType = typeof existingSelector === 'string'
    ? existingSelector
    : existingSelector.selectorType
  if (existingSelectorType !== 'version') {
    throw new Error(`Encountered unexpected version selector '${existingSelectorType}' for dependency '${name}@${version}'`)
  }

  selectors[version] = addWeightToVersionSelector(existingSelector, weight)
}

function addWeightToVersionSelector (
  selector: VersionSelectorWithWeight | VersionSelectorType,
  weight: number
): VersionSelectorWithWeight {
  return typeof selector === 'string'
    ? { selectorType: selector, weight: weight + 1 }
    : { selectorType: selector.selectorType, weight: selector.weight + weight }
}

function collectUniqueNameVersions (snapshots: PackageSnapshots): Record<string, Set<string>> {
  const uniqueNameVersions: Record<string, Set<string>> = Object.create(null)
  for (const [depPath, snapshot] of Object.entries(snapshots)) {
    const { name, version } = nameVerFromPkgSnapshot(depPath, snapshot)
    uniqueNameVersions[name] ??= new Set()
    uniqueNameVersions[name].add(version)
  }
  return uniqueNameVersions
}

