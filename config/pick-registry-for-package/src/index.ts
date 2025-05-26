import { type Registries } from '@pnpm/types'

export function pickRegistryForPackage (registries: Registries, packageName: string, bareSpecifier?: string): string {
  const scope = getScope(packageName, bareSpecifier)
  return (scope && registries[scope]) ?? registries.default
}

function getScope (pkgName: string, bareSpecifier?: string): string | null {
  if (bareSpecifier?.startsWith('npm:')) {
    bareSpecifier = bareSpecifier.slice(4)
    if (bareSpecifier[0] === '@') {
      return bareSpecifier.substring(0, bareSpecifier.indexOf('/'))
    }
  }
  if (pkgName[0] === '@') {
    return pkgName.substring(0, pkgName.indexOf('/'))
  }
  return null
}
