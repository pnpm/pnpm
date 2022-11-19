import parseNpmTarballUrl from 'parse-npm-tarball-url'
import getVersionSelectorType from 'version-selector-type'

export interface RegistryPackageSpec {
  type: 'tag' | 'version' | 'range'
  name: string
  fetchSpec: string
  normalizedPref?: string
}

export function parsePref (
  pref: string,
  alias: string | undefined,
  defaultTag: string,
  registry: string
): RegistryPackageSpec | null {
  let name = alias
  if (pref.startsWith('npm:')) {
    pref = pref.slice(4)
    const index = pref.lastIndexOf('@')
    if (index < 1) {
      name = pref
      pref = defaultTag
    } else {
      name = pref.slice(0, index)
      pref = pref.slice(index + 1)
    }
  }
  if (name) {
    const selector = getVersionSelectorType(pref)
    if (selector != null) {
      return {
        fetchSpec: selector.normalized,
        name,
        type: selector.type,
      }
    }
  }
  if (pref.startsWith(registry)) {
    const pkg = parseNpmTarballUrl(pref)
    if (pkg != null) {
      return {
        fetchSpec: pkg.version,
        name: pkg.name,
        normalizedPref: pref,
        type: 'version',
      }
    }
  }
  return null
}
