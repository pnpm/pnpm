import { nameVerFromPkgSnapshot, PackageSnapshots } from '@pnpm/lockfile-utils'
import { PreferredVersions } from '@pnpm/resolver-base'
import { Dependencies, ProjectManifest } from '@pnpm/types'
import { getAllDependenciesFromPackage } from '@pnpm/utils'
import getVerSelType = require('version-selector-type')

export default function getPreferredVersionsFromPackage (
  pkg: Pick<ProjectManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies'>,
): PreferredVersions {
  return getVersionSpecsByRealNames(getAllDependenciesFromPackage(pkg))
}

function getVersionSpecsByRealNames (deps: Dependencies) {
  return Object.keys(deps)
    .reduce((acc, depName) => {
      if (deps[depName].startsWith('npm:')) {
        const pref = deps[depName].substr(4)
        const index = pref.lastIndexOf('@')
        const spec = pref.substr(index + 1)
        const selector = getVerSelType(spec)
        if (selector) {
          const pkgName = pref.substr(0, index)
          acc[pkgName] = acc[pkgName] || {}
          acc[pkgName][selector.normalized] = selector.type
        }
      } else if (!deps[depName].includes(':')) { // we really care only about semver specs
        const selector = getVerSelType(deps[depName])
        if (selector) {
          acc[depName] = acc[depName] || {}
          acc[depName][selector.normalized] = selector.type
        }
      }
      return acc
    }, {} as PreferredVersions)
}

export function getPreferredVersionsFromLockfile (snapshots: PackageSnapshots): PreferredVersions {
  const preferredVersions: PreferredVersions = {}
  for (const [relDepPath, snapshot] of Object.entries(snapshots)) {
    const { name, version } = nameVerFromPkgSnapshot(relDepPath, snapshot)
    if (!preferredVersions[name]) {
      preferredVersions[name] = { [version]: 'version' }
    } else {
      preferredVersions[name][version] = 'version'
    }
  }
  return preferredVersions
}
