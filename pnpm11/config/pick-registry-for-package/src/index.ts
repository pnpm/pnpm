import type { RegistriesByPrefix, RegistriesByScope } from '@pnpm/types'

export function pickRegistryForPackage (registriesByScope: RegistriesByScope, packageName: string, bareSpecifier?: string): string {
  const scope = getScope(packageName, bareSpecifier)
  return (scope && registriesByScope[scope]) ?? registriesByScope.default
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

/**
 * URL prefixes a recorded tarball URL is matched against to pick the registry
 * to verify it with, longest first.
 *
 * Adding a built-in registry is therefore not a local change: it also decides
 * where verification traffic goes for entries that name no registry. Memoized
 * on the map because the caller sits behind a per-package walk.
 */
export function namedRegistryTarballPrefixes (registriesByPrefix: RegistriesByPrefix): readonly string[] {
  let prefixes = tarballPrefixesCache.get(registriesByPrefix)
  if (prefixes) return prefixes

  prefixes = Object.values(registriesByPrefix)
    .map((url) => {
      let parsed: URL
      try {
        parsed = new URL(url)
      } catch {
        return null
      }
      // Trailing slash so `https://npm.pkg.github.com-evil/` cannot match.
      const pathname = parsed.pathname.endsWith('/') ? parsed.pathname : `${parsed.pathname}/`
      return `${parsed.origin}${pathname}`
    })
    .filter((prefix): prefix is string => prefix != null)
    // Tie-break equal lengths so the order does not depend on key order.
    .sort((a, b) => b.length - a.length || (a < b ? -1 : a > b ? 1 : 0))
  tarballPrefixesCache.set(registriesByPrefix, prefixes)
  return prefixes
}

const tarballPrefixesCache = new WeakMap<RegistriesByPrefix, readonly string[]>()
