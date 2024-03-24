import getVersionSelectorType from 'version-selector-type'

import {
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile-utils'
import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import type { DependencyManifest, PackageSnapshots, PreferredVersions, ProjectManifest } from '@pnpm/types'

export function getAllUniqueSpecs(manifests: DependencyManifest[]) {
  const allSpecs: Record<string, string> = {}

  const ignored = new Set<string>()

  for (const manifest of manifests) {
    const specs = getAllDependenciesFromManifest(manifest)

    for (const [name, spec] of Object.entries(specs)) {
      if (ignored.has(name)) {
        continue
      }

      if (
        (allSpecs[name] != null && allSpecs[name] !== spec) ||
        spec.includes(':')
      ) {
        ignored.add(name)
        delete allSpecs[name]
        continue
      }

      allSpecs[name] = spec
    }
  }

  return allSpecs
}

export function getPreferredVersionsFromLockfileAndManifests(
  snapshots: PackageSnapshots | undefined,
  manifests: Array<DependencyManifest | ProjectManifest>
): PreferredVersions {
  const preferredVersions: PreferredVersions = {}

  for (const manifest of manifests) {
    const specs = getAllDependenciesFromManifest(manifest)

    for (const [name, spec] of Object.entries(specs)) {
      const selector = getVersionSelectorType(spec)

      if (!selector) {
        continue
      }

      preferredVersions[name] = preferredVersions[name] ?? {}

      preferredVersions[name][spec] = {
        selectorType: selector.type,
        weight: 1000,
      }
    }
  }

  if (!snapshots) {
    return preferredVersions
  }

  addPreferredVersionsFromLockfile(snapshots, preferredVersions)

  return preferredVersions
}

function addPreferredVersionsFromLockfile(
  snapshots: PackageSnapshots,
  preferredVersions: PreferredVersions
) {
  for (const [depPath, snapshot] of Object.entries(snapshots)) {
    const { name, version } = nameVerFromPkgSnapshot(depPath, snapshot)

    if (!preferredVersions[name]) {
      preferredVersions[name] = { [version]: 'version' }
    } else if (!preferredVersions[name][version]) {
      preferredVersions[name][version] = 'version'
    }
  }
}
