import {PackageJson, Dependencies} from '@pnpm/types'
import getVerSelType = require('version-selector-type')

export default function depsFromPackage (pkg: PackageJson): Dependencies {
  return {
    ...pkg.devDependencies,
    ...pkg.dependencies,
    ...pkg.optionalDependencies
  } as Dependencies
}

export function getPreferredVersionsFromPackage (pkg: PackageJson): {
  [packageName: string]: {
    type: 'version' | 'range' | 'tag',
    selector: string,
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
        const selector = pref.substr(index + 1)
        const type = getVerSelType(selector)
        if (type) {
          acc[pref.substr(0, index)] = {
            type,
            selector,
          }
        }
      } else if (deps[depName].indexOf(':') === -1) { // we really care only about semver specs
        const type = getVerSelType(deps[depName])
        if (type) {
          acc[depName] = {
            type,
            selector: deps[depName],
          }
        }
      }
      return acc
    }, {})
}
