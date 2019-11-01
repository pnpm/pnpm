import { Registries } from '@pnpm/types'

export default (registries: Registries, packageName: string) => {
  const scope = getScope(packageName)
  return (scope && registries[scope]) ?? registries.default
}

function getScope (pkgName: string): string | null {
  if (pkgName[0] === '@') {
    return pkgName.substr(0, pkgName.indexOf('/'))
  }
  return null
}
