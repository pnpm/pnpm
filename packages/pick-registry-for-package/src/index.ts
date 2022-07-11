import { Registries } from '@pnpm/types'

export default (registries: Registries, packageName: string, pref?: string) => {
  const scope = getScope(packageName, pref)
  return (scope && registries[scope]) ?? registries.default
}

function getScope (pkgName: string, pref?: string): string | null {
  if (pref?.startsWith('npm:')) {
    pref = pref.slice(4)
    if (pref[0] === '@') {
      return pref.substring(0, pref.indexOf('/'))
    }
  }
  if (pkgName[0] === '@') {
    return pkgName.substring(0, pkgName.indexOf('/'))
  }
  return null
}
