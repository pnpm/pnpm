import type { Registries } from '@pnpm/types'

export function pickRegistryForPackage(
  registries: Registries,
  packageName: string,
  pref?: string | undefined
): string {
  const scope = getScope(packageName, pref)

  return (scope && registries[scope]) ?? registries.default
}

function getScope(pkgName: string, pref?: string | undefined): string | null {
  if (pref?.startsWith('npm:')) {
    pref = pref.slice(4)

    if (pref.startsWith('@')) {
      return pref.substring(0, pref.indexOf('/'))
    }
  }

  if (pkgName.startsWith('@')) {
    return pkgName.substring(0, pkgName.indexOf('/'))
  }

  return null
}
