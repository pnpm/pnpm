import { BUILTIN_NAMED_REGISTRIES } from '@pnpm/constants'
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

/**
 * Every registry pnpm can route to by alias, and the two ways that set is
 * consulted. Build it once per install and pass it down: the set is the
 * single place the built-ins and the user's `namedRegistries` are combined,
 * so a change to either cannot alter one consumer without the other.
 */
export interface KnownRegistries {
  /**
   * Alias → registry URL, as a null-prototype record.
   *
   * The prototype matters: alias names come out of the lockfile's dep paths,
   * so a crafted `foo@constructor:1.0.0` would otherwise look up
   * `Object.prototype.constructor` — a truthy function — and sail past every
   * `if (!registry)` guard that is there to fail closed on an unknown alias.
   * Always resolve an alias through this, never through a plain object literal.
   */
  readonly byAlias: Record<string, string>
  /**
   * The URL prefixes a recorded tarball URL is matched against to decide
   * which registry to verify an entry with, longest first so the deepest
   * match wins.
   *
   * This is why adding an entry to {@link BUILTIN_NAMED_REGISTRIES} is not a
   * local change: it also decides where verification traffic goes for
   * lockfile entries that name no alias at all. Every prefix ends in `/` so
   * matching cannot be fooled by a same-host-different-suffix sibling
   * (`https://npm.pkg.github.com-evil/`).
   */
  readonly tarballPrefixes: readonly string[]
}

/**
 * Combine the built-in aliases with the user's `namedRegistries` (user wins
 * on collision, so a GHES user can point `gh` at their enterprise host).
 *
 * `tarballPrefixes` is computed on first access: the alias lookup runs per
 * package, while prefix matching runs only when an entry carries a recorded
 * tarball URL.
 */
export function createKnownRegistries (namedRegistries?: Record<string, string>): KnownRegistries {
  const byAlias: Record<string, string> = Object.assign(
    Object.create(null) as Record<string, string>,
    BUILTIN_NAMED_REGISTRIES,
    namedRegistries
  )
  let tarballPrefixes: readonly string[] | undefined
  return {
    byAlias,
    get tarballPrefixes (): readonly string[] {
      tarballPrefixes ??= buildTarballPrefixes(byAlias)
      return tarballPrefixes
    },
  }
}

function buildTarballPrefixes (byAlias: Record<string, string>): readonly string[] {
  return Object.values(byAlias)
    .map((url) => {
      let parsed: URL
      try {
        parsed = new URL(url)
      } catch {
        return null
      }
      const pathname = parsed.pathname.endsWith('/') ? parsed.pathname : `${parsed.pathname}/`
      return `${parsed.origin}${pathname}`
    })
    .filter((prefix): prefix is string => prefix != null)
    // Equal-length prefixes tie-break lexicographically so the order is
    // total, matching the Rust stack and keeping assertions on it stable.
    .sort((a, b) => b.length - a.length || (a < b ? -1 : a > b ? 1 : 0))
}
