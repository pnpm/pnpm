import { nameVerFromPkgSnapshot, type PackageSnapshots } from '@pnpm/lockfile.utils'
import { getAllDependenciesFromManifest } from '@pnpm/pkg-manifest.utils'
import {
  DIRECT_DEP_SELECTOR_WEIGHT,
  EXISTING_VERSION_SELECTOR_WEIGHT,
  type PreferredVersions,
  type VersionSelectorType,
  type VersionSelectorWithWeight,
} from '@pnpm/resolving.resolver-base'
import type { DependencyManifest, ProjectManifest } from '@pnpm/types'
import getVersionSelectorType from 'version-selector-type'

export function getPreferredVersionsFromLockfileAndManifests (
  snapshots: PackageSnapshots | undefined,
  manifests: Array<DependencyManifest | ProjectManifest>
): PreferredVersions {
  const preferredVersions: PreferredVersions = {}
  for (const manifest of manifests) {
    const specs = getAllDependenciesFromManifest(manifest)
    for (const [name, spec] of Object.entries(specs)) {
      const selector = getVersionSelectorType(spec)
      if (!selector) continue
      preferredVersions[name] = preferredVersions[name] ?? {}
      preferredVersions[name][spec] = {
        selectorType: selector.type,
        weight: DIRECT_DEP_SELECTOR_WEIGHT,
      }
    }
  }
  if (!snapshots) return preferredVersions
  addPreferredVersionsFromLockfile(snapshots, preferredVersions)
  return preferredVersions
}

function addPreferredVersionsFromLockfile (snapshots: PackageSnapshots, preferredVersions: PreferredVersions): void {
  // The snapshots object can contain multiple entries with the same package
  // name and version. This is because a dependency can appear multiple times
  // with the same version in the lockfile due to peer dependency resolution. To
  // avoid inflating the weight of package versions that appear multiple times,
  // generate a map with only the unique set to iterate over.
  const uniqueNameVersions: Record<string, Set<string>> = {}
  for (const [depPath, snapshot] of Object.entries(snapshots)) {
    const { name, version } = nameVerFromPkgSnapshot(depPath, snapshot)
    uniqueNameVersions[name] ??= new Set()
    uniqueNameVersions[name].add(version)
  }

  for (const [name, versions] of Object.entries(uniqueNameVersions)) {
    for (const version of versions) {
      preferredVersions[name] ??= {}

      const existingSelector = preferredVersions[name][version]
      if (existingSelector == null) {
        preferredVersions[name][version] = { selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT }
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
      preferredVersions[name][version] = addWeightToVersionSelector(existingSelector, EXISTING_VERSION_SELECTOR_WEIGHT)
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
