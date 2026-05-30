import type { Registries } from '@pnpm/types'

export function pickRegistryForPackage (registries: Registries, packageName: string, bareSpecifier?: string): string {
  const scope = getScope(packageName, bareSpecifier)
  return (scope && registries[scope]) ?? registries.default
}

function getScope (pkgName: string, bareSpecifier?: string): string | null {
  if (bareSpecifier?.startsWith('npm:')) {
    const target = bareSpecifier.slice(4)
    if (target[0] === '@') {
      return target.substring(0, target.indexOf('/'))
    }
    // Unscoped `npm:` alias target (e.g. `"@private/foo": "npm:lodash@^1"`).
    // The package being fetched is unscoped, so the local alias's scope must
    // not drive registry routing — `lodash` doesn't live on the `@private`
    // registry. Fall through to the default registry instead.
    return null
  }
  if (pkgName[0] === '@') {
    return pkgName.substring(0, pkgName.indexOf('/'))
  }
  return null
}
