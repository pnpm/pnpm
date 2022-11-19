import { nameVerFromPkgSnapshot, PackageSnapshots } from '@pnpm/lockfile-utils'
import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import { PreferredVersions } from '@pnpm/resolver-base'
import { DependencyManifest } from '@pnpm/types'

export function getAllUniqueSpecs (manifests: DependencyManifest[]) {
  const allSpecs: Record<string, string> = {}
  const ignored = new Set<string>()
  for (const manifest of manifests) {
    const specs = getAllDependenciesFromManifest(manifest)
    for (const [name, spec] of Object.entries(specs)) {
      if (ignored.has(name)) continue
      if (allSpecs[name] != null && allSpecs[name] !== spec || spec.includes(':')) {
        ignored.add(name)
        delete allSpecs[name]
        continue
      }
      allSpecs[name] = spec
    }
  }
  return allSpecs
}

export function getPreferredVersionsFromLockfile (snapshots: PackageSnapshots): PreferredVersions {
  return Object.entries(snapshots)
    .map(([depPath, snapshot]) => nameVerFromPkgSnapshot(depPath, snapshot))
    .reduce((preferredVersions, { name, version }) => {
      if (!preferredVersions[name]) {
        preferredVersions[name] = { [version]: 'version' }
      } else {
        preferredVersions[name][version] = 'version'
      }
      return preferredVersions
    }, {})
}
