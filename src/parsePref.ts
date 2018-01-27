import semver = require('semver')
import getVersionSelectorType = require('version-selector-type')

export interface RegistryPackageSpec {
  type: 'tag' | 'version' | 'range',
  name: string,
  fetchSpec: string,
}

export default function parsePref (pref: string, alias?: string): RegistryPackageSpec | null {
  let name = alias
  if (pref.startsWith('npm:')) {
    pref = pref.substr(4)
    const index = pref.lastIndexOf('@')
    if (index < 1) {
      name = pref
      pref = 'latest'
    } else {
      name = pref.substr(0, index)
      pref = pref.substr(index + 1)
    }
  }
  if (!name) {
    return null
  }
  const selector = getVersionSelectorType(pref)
  if (selector) {
    return {
      fetchSpec: selector.normalized,
      name,
      type: selector.type,
    }
  }
  return null
}
