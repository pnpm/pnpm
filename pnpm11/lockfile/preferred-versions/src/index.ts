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
  // All maps in here are keyed by package names and specifiers coming from
  // manifests and the lockfile — attacker-controlled inputs. Null-prototype
  // objects make a crafted key (e.g. `__proto__`) a plain own key instead of
  // a write through Object.prototype.
  const preferredVersions: PreferredVersions = Object.create(null)
  for (const manifest of manifests) {
    const specs = getAllDependenciesFromManifest(manifest)
    for (const [name, bareSpecifier] of Object.entries(specs)) {
      const spec = resolveCatalogSpec(opts.catalogs ?? {}, name, bareSpecifier)
      if (spec == null) continue
      const selector = getVersionSelectorType(spec)
      if (!selector) continue
      preferredVersions[name] = preferredVersions[name] ?? (Object.create(null) as VersionSelectors)
      preferredVersions[name][spec] = {
        selectorType: selector.type,
        weight: DIRECT_DEP_SELECTOR_WEIGHT,
      }
    }
  }
  if (!snapshots) return preferredVersions
  // Dedupe must let newly resolved dependencies compete with lockfile versions.
  addPreferredVersionsFromLockfile(snapshots, preferredVersions, opts.dedupe ? 1 : EXISTING_VERSION_SELECTOR_WEIGHT)
  return preferredVersions
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
  // The snapshots object can contain multiple entries with the same package
  // name and version. This is because a dependency can appear multiple times
  // with the same version in the lockfile due to peer dependency resolution. To
  // avoid inflating the weight of package versions that appear multiple times,
  // generate a map with only the unique set to iterate over.
  const uniqueNameVersions: Record<string, Set<string>> = Object.create(null)
  for (const [depPath, snapshot] of Object.entries(snapshots)) {
    const { name, version } = nameVerFromPkgSnapshot(depPath, snapshot)
    uniqueNameVersions[name] ??= new Set()
    uniqueNameVersions[name].add(version)
  }

  for (const [name, versions] of Object.entries(uniqueNameVersions)) {
    for (const version of versions) {
      preferredVersions[name] ??= Object.create(null) as VersionSelectors

      const existingSelector = preferredVersions[name][version]
      if (existingSelector == null) {
        preferredVersions[name][version] = { selectorType: 'version', weight }
        continue
      }

      // The lookup for this selector was for an exact version and not a range
      // or tag. If there's an existing selector and it's not for a version,
      // that's unexpected and our program state is corrupted.
      const existingSelectorType = typeof existingSelector === 'string'
        ? existingSelector
        : existingSelector.selectorType
      if (existingSelectorType !== 'version') {
        throw new Error(`Encountered unexpected version selector '${existingSelectorType}' for dependency '${name}@${version}'`)
      }

      // There might be an existing selector on this exact version from a direct
      // dependency. If so, we should increase its weight. This allows a version
      // present in the lockfile that's also used by a direct dependency to be
      // considered at a higher priority than a package with only one of the two
      // criteria.
      preferredVersions[name][version] = addWeightToVersionSelector(existingSelector, weight)
    }
  }
}

function addWeightToVersionSelector (
  selector: VersionSelectorWithWeight | VersionSelectorType,
  weight: number
): VersionSelectorWithWeight {
  return typeof selector === 'string'
    ? { selectorType: selector, weight: weight + 1 }
    : { selectorType: selector.selectorType, weight: selector.weight + weight }
}
