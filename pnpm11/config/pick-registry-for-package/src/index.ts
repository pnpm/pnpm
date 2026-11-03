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
 * The registries that decide the prefix a lockfile key carries:
 * `registriesByScope` routes plain ranges, and its `default` registry keeps
 * plain keys. `registriesByPrefix` is the prefix map as the user wrote it,
 * without the built-in aliases.
 */
export interface KeyPrefixRegistries {
  registriesByScope: RegistriesByScope
  registriesByPrefix?: Record<string, string>
}

/**
 * The prefix the lockfile key of a package resolved from `registry` carries,
 * so the key names the registry the package resolved from rather than the way
 * its dependency was written.
 *
 * Only a prefix the project declares qualifies keys, through
 * `registries.<url>.prefix` or `namedRegistries`. A prefix that names the
 * default registry qualifies nothing: the packages of the default registry
 * keep plain keys. When several declared prefixes name one registry, the
 * lexicographically smallest one is picked. A prefix whose URL does not parse
 * names no registry.
 */
export function pickDeclaredRegistryPrefix (registries: KeyPrefixRegistries, registry: string): string | undefined {
  const declared = registries.registriesByPrefix
  // Asked once per resolved package, and most projects declare no prefix.
  if (declared == null || Object.keys(declared).length === 0) return undefined
  const registryKey = normalizeRegistryKey(registry)
  if (registryKey == null) return undefined
  const prefix = getDeclaredPrefixByRegistry(declared).get(registryKey)
  if (prefix == null || registryKey === normalizeDefaultRegistry(registries.registriesByScope.default)) return undefined
  return prefix
}

/**
 * The prefix the lockfile key of `name`, resolved by an `<alias>:` specifier
 * from `registry`, carries. `undefined` is a plain key.
 *
 * A plain key reads as resolved from the registry a plain range of `name`
 * routes to, so the key is plain when `registry` is both the default registry
 * and that one. Otherwise it carries the smallest prefix declared for
 * `registry`, the default registry included, or `alias` when none is.
 */
export function pickAliasKeyPrefix (
  registries: KeyPrefixRegistries,
  { alias, name, registry }: { alias: string, name: string, registry: string }
): string | undefined {
  const defaultRegistry = registries.registriesByScope.default
  if (isSameRegistry(registry, defaultRegistry) && isSameRegistry(pickRegistryForPackage(registries.registriesByScope, name), defaultRegistry)) {
    return undefined
  }
  const declared = registries.registriesByPrefix
  const registryKey = normalizeRegistryKey(registry)
  const declaredPrefix = declared == null || registryKey == null ? undefined : getDeclaredPrefixByRegistry(declared).get(registryKey)
  return declaredPrefix ?? alias
}

function normalizeDefaultRegistry (defaultRegistry: string): string | undefined {
  if (!normalizedDefaultRegistryCache.has(defaultRegistry)) {
    normalizedDefaultRegistryCache.set(defaultRegistry, normalizeRegistryKey(defaultRegistry))
  }
  return normalizedDefaultRegistryCache.get(defaultRegistry)
}

// The default registry is one URL per install, so the cache stays tiny.
const normalizedDefaultRegistryCache = new Map<string, string | undefined>()

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
