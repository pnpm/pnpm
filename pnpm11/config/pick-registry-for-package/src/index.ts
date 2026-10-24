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
    // The key ends in a slash, so `https://npm.pkg.github.com-evil/` cannot match.
    .map((url) => normalizeRegistryKey(url))
    .filter((prefix): prefix is string => prefix != null)
    // Tie-break equal lengths so the order does not depend on key order.
    .sort((a, b) => b.length - a.length || (a < b ? -1 : a > b ? 1 : 0))
  tarballPrefixesCache.set(registriesByPrefix, prefixes)
  return prefixes
}

const tarballPrefixesCache = new WeakMap<RegistriesByPrefix, readonly string[]>()

/**
 * A registry URL as the server sees it, for telling two spellings of one
 * registry from two registries: the scheme and host case-insensitive, the
 * path as written and ending in a slash. `undefined` for a URL that does not
 * parse.
 */
export function normalizeRegistryKey (registry: string): string | undefined {
  let parsed: URL
  try {
    parsed = new URL(registry)
  } catch {
    return undefined
  }
  const pathname = parsed.pathname.endsWith('/') ? parsed.pathname : `${parsed.pathname}/`
  return `${parsed.origin}${pathname}`
}

/**
 * Whether two registry URLs name one registry, compared by
 * {@link normalizeRegistryKey}, or as written when either does not parse.
 */
export function isSameRegistry (left: string, right: string): boolean {
  const leftKey = normalizeRegistryKey(left)
  const rightKey = normalizeRegistryKey(right)
  return leftKey != null && rightKey != null ? leftKey === rightKey : left === right
}

/**
 * The prefix the lockfile key of a package resolved from `registry` carries,
 * so the key names the registry the package resolved from rather than the way
 * its dependency was written.
 *
 * Only a prefix the project declares qualifies keys, through
 * `registries.<url>.prefix` or `namedRegistries`: `declaredRegistriesByPrefix`
 * is that map as the user wrote it, without the built-in aliases, which
 * qualify only the specifiers that spell them. When several declared prefixes
 * name one registry, the lexicographically smallest one is picked. A prefix
 * whose URL does not parse names no registry.
 */
export function pickDeclaredRegistryPrefix (
  declaredRegistriesByPrefix: Record<string, string> | undefined,
  registry: string
): string | undefined {
  // Asked once per resolved package, and most projects declare no prefix.
  if (declaredRegistriesByPrefix == null || Object.keys(declaredRegistriesByPrefix).length === 0) return undefined
  const registryKey = normalizeRegistryKey(registry)
  if (registryKey == null) return undefined
  return getDeclaredPrefixByRegistry(declaredRegistriesByPrefix).get(registryKey)
}

function getDeclaredPrefixByRegistry (declaredRegistriesByPrefix: Record<string, string>): ReadonlyMap<string, string> {
  let prefixByRegistry = declaredPrefixByRegistryCache.get(declaredRegistriesByPrefix)
  if (prefixByRegistry != null) return prefixByRegistry
  const smallestPrefixByRegistry = new Map<string, string>()
  for (const [prefix, registry] of Object.entries(declaredRegistriesByPrefix)) {
    const registryKey = normalizeRegistryKey(registry)
    if (registryKey == null) continue
    const smallest = smallestPrefixByRegistry.get(registryKey)
    if (smallest == null || prefix < smallest) {
      smallestPrefixByRegistry.set(registryKey, prefix)
    }
  }
  prefixByRegistry = smallestPrefixByRegistry
  declaredPrefixByRegistryCache.set(declaredRegistriesByPrefix, prefixByRegistry)
  return prefixByRegistry
}

const declaredPrefixByRegistryCache = new WeakMap<Record<string, string>, ReadonlyMap<string, string>>()
