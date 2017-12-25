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
    name = pref.substr(0, index)
    pref = pref.substr(index + 1)
  }
  if (!name) {
    return null
  }
  // TODO: this should also return the clean version of the spec (what semver.valid returns)
  const type = getVersionSelectorType(pref, true)
  if (type) {
    return {
      fetchSpec: pref,
      name,
      type,
    }
  }
  return null
}
