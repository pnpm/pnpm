import { nameVerFromPkgSnapshot, PackageSnapshots } from '@pnpm/lockfile-utils'
import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import { PreferredVersions } from '@pnpm/resolver-base'
import { Dependencies, DependencyManifest, ProjectManifest } from '@pnpm/types'
import getVerSelType from 'version-selector-type'

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

export default function getPreferredVersionsFromPackage (
  pkg: Pick<ProjectManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies'>
): PreferredVersions {
  return getVersionSpecsByRealNames(getAllDependenciesFromManifest(pkg))
}

function getVersionSpecsByRealNames (deps: Dependencies) {
  return Object.keys(deps)
    .reduce((acc, depName) => {
      if (deps[depName].startsWith('npm:')) {
        const pref = deps[depName].substr(4)
        const index = pref.lastIndexOf('@')
        const spec = pref.substr(index + 1)
        const selector = getVerSelType(spec)
        if (selector != null) {
          const pkgName = pref.substr(0, index)
          acc[pkgName] = acc[pkgName] || {}
          acc[pkgName][selector.normalized] = selector.type
        }
      } else if (!deps[depName].includes(':')) { // we really care only about semver specs
        const selector = getVerSelType(deps[depName])
        if (selector != null) {
          acc[depName] = acc[depName] || {}
          acc[depName][selector.normalized] = selector.type
        }
      }
      return acc
    }, {})
}

export function getPreferredVersionsFromLockfile (snapshots: PackageSnapshots): PreferredVersions {
  const preferredVersions: PreferredVersions = {}
  for (const [depPath, snapshot] of Object.entries(snapshots)) {
    const { name, version } = nameVerFromPkgSnapshot(depPath, snapshot)
    if (!preferredVersions[name]) {
      preferredVersions[name] = { [version]: 'version' }
    } else {
      preferredVersions[name][version] = 'version'
    }
  }
  return preferredVersions
}
