import { PreferredVersions } from '@pnpm/resolver-base'
import { Dependencies, ImporterManifest } from '@pnpm/types'
import { getAllDependenciesFromPackage } from '@pnpm/utils'
import getVerSelType = require('version-selector-type')

export default function getPreferredVersionsFromPackage (
  pkg: Pick<ImporterManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies'>,
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
