import type { Registries } from '@pnpm/types'

/**
 * Picks the registry URL that should be used to resolve the given package.
 *
 * Resolution order:
 *   1. An exact match in `registryOverrides` (keyed by the real package name,
 *      derived from `npm:` aliases when applicable) wins. This is how the
 *      `registryOverrides` setting lets a single package in a scope be served
 *      from a different registry than the rest of the scope.
 *   2. Otherwise, if the package is scoped and the scope is a key in
 *      `registries`, the scope's registry is returned.
 *   3. Otherwise, `registries.default` is returned.
 *
 * Note: this function only decides the registry URL. It does not run before
 * custom resolvers configured in `.pnpmfile.cjs` — those still take precedence
 * in the overall resolution chain (see `@pnpm/resolving.default-resolver`).
 * Authentication for the returned URL is looked up separately by
 * `createGetAuthHeaderByURI` using the existing per-URL `.npmrc` entries.
 */
export function pickRegistryForPackage (
  registries: Registries,
  packageName: string,
  bareSpecifier?: string,
  registryOverrides?: Record<string, string>
): string {
  const realName = getRealPackageName(packageName, bareSpecifier)
  if (registryOverrides?.[realName]) return registryOverrides[realName]
  const scope = getScopeFromName(realName)
  return (scope && registries[scope]) ?? registries.default
}

/**
 * Returns the canonical package name for a dependency. For `npm:`-aliased
 * specifiers (e.g. `npm:@foo/pkg@1.2.3`), this extracts the original name
 * (`@foo/pkg`) so that the override lookup and scope lookup match the real
 * package rather than the local alias.
 */
function getRealPackageName (pkgName: string, bareSpecifier?: string): string {
  if (bareSpecifier?.startsWith('npm:')) {
    const rest = bareSpecifier.slice(4)
    if (rest === '') return pkgName
    // Strip the version tail: "@foo/pkg@1.2.3" -> "@foo/pkg", "pkg@1.2.3" -> "pkg".
    // In a scoped name, the name-version separator is the second '@'.
    const versionAt = rest.indexOf('@', rest[0] === '@' ? 1 : 0)
    if (versionAt === 0) return pkgName
    return versionAt > 0 ? rest.slice(0, versionAt) : rest
  }
  return pkgName
}

function getScopeFromName (pkgName: string): string | null {
  if (pkgName[0] !== '@') return null
  const slashIdx = pkgName.indexOf('/')
  return slashIdx > 0 ? pkgName.substring(0, slashIdx) : null
}
