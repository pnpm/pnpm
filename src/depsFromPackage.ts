import {Dependencies, PackageJson} from '@pnpm/types'
import getVerSelType = require('version-selector-type')

export default function depsFromPackage (pkg: PackageJson): Dependencies {
  return {
    ...pkg.devDependencies,
    ...pkg.dependencies,
    ...pkg.optionalDependencies,
  } as Dependencies
}

export function getPreferredVersionsFromPackage (pkg: PackageJson): {
  [packageName: string]: {
    selector: string,
    type: 'version' | 'range' | 'tag',
  },
} {
  return getVersionSpecsByRealNames(depsFromPackage(pkg))
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
          acc[pref.substr(0, index)] = {
            selector: selector.normalized,
            type: selector.type,
          }
        }
      } else if (deps[depName].indexOf(':') === -1) { // we really care only about semver specs
        const selector = getVerSelType(deps[depName])
        if (selector) {
          acc[depName] = {
            selector: selector.normalized,
            type: selector.type,
          }
        }
      }
      return acc
    }, {})
}
