import type { NamedRegistries, Registries } from '@pnpm/types'

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
 * Every registry pnpm can route to by name, and the two ways that set is
 * consulted. Built once per registry map, so name lookup and reverse routing
 * can never disagree about which registries exist.
 */
export interface KnownRegistries {
  /**
   * Registry name → registry URL, as a null-prototype record.
   *
   * The prototype matters: registry names come out of the lockfile's dep paths,
   * so a crafted `foo@constructor:1.0.0` would otherwise look up
   * `Object.prototype.constructor` — a truthy function — and sail past every
   * `if (!registry)` guard that is there to fail closed on an unknown name.
   * Always resolve a name through this, never through a plain object literal.
   */
  readonly byName: Record<string, string>
  /**
   * The URL prefixes a recorded tarball URL is matched against to decide
   * which registry to verify an entry with, longest first so the deepest
   * match wins.
   *
   * This is why adding a built-in registry is not a
   * local change: it also decides where verification traffic goes for
   * lockfile entries that name no registry at all. Every prefix ends in `/` so
   * matching cannot be fooled by a same-host-different-suffix sibling
   * (`https://npm.pkg.github.com-evil/`).
   */
  readonly tarballPrefixes: readonly string[]
}

/**
 * One instance per registry map, cached on the map itself.
 *
 * Callers sit in per-package loops — the dep-graph builders, the SBOM walk,
 * the license scanner — and thread `namedRegistries` down rather than a built
 * instance. Keying the cache on the map keeps "one set per install" true
 * without every one of those call sites having to remember to hoist, which is
 * the kind of thing that silently regresses. The map comes from
 * `normalizeNamedRegistries` and is not mutated afterwards.
 *
 * The built-ins are merged in by `normalizeNamedRegistries` at the boundary,
 * which is why `NamedRegistries` is required here rather than a raw record:
 * a map that never passed through it does not typecheck.
 *
 * `tarballPrefixes` is computed on first access: the name lookup runs per
 * package, while prefix matching runs only when an entry carries a recorded
 * tarball URL.
 */
export function createKnownRegistries (namedRegistries: NamedRegistries): KnownRegistries {
  const cached = knownRegistriesCache.get(namedRegistries)
  if (cached) return cached

  const byName: Record<string, string> = Object.assign(
    Object.create(null) as Record<string, string>,
    namedRegistries
  )
  let tarballPrefixes: readonly string[] | undefined
  const knownRegistries: KnownRegistries = {
    byName,
    get tarballPrefixes (): readonly string[] {
      tarballPrefixes ??= buildTarballPrefixes(byName)
      return tarballPrefixes
    },
  }
  knownRegistriesCache.set(namedRegistries, knownRegistries)
  return knownRegistries
}

const knownRegistriesCache = new WeakMap<NamedRegistries, KnownRegistries>()

function buildTarballPrefixes (byName: Record<string, string>): readonly string[] {
  return Object.values(byName)
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
