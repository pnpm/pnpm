import semver = require('semver')

export interface RegistryPackageSpec {
  type: 'tag' | 'version' | 'range',
  name: string,
  fetchSpec: string,
}

export default function parsePref (pref: string, alias?: string): RegistryPackageSpec | null {
  let name = alias
  if (pref.startsWith('npm:')) {
    pref = pref.substr(4)
    const parts = pref.split('@')
    name = parts[0]
    pref = parts[1]
  }
  if (!name) {
    return null
  }
  if (semver.valid(pref, true)) {
    return {
      fetchSpec: pref,
      name,
      type: 'version',
    }
  }
  if (semver.validRange(pref, true)) {
    return {
      fetchSpec: pref,
      name,
      type: 'range',
    }
  }
  if (encodeURIComponent(pref) === pref) {
    return {
      fetchSpec: pref,
      name,
      type: 'tag',
    }
  }
  return null
}
