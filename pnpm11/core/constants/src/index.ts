export const WANTED_LOCKFILE = 'pnpm-lock.yaml'
export const LOCKFILE_MAJOR_VERSION = '9'
export const LOCKFILE_VERSION = `${LOCKFILE_MAJOR_VERSION}.0`

export const MANIFEST_BASE_NAMES = ['package.json', 'package.json5', 'package.yaml'] as const

export const ENGINE_NAME = `${process.platform};${process.arch};node${process.version.split('.')[0].substring(1)}`
export const LAYOUT_VERSION = 5
export const STORE_VERSION = 'v11'
export const GLOBAL_LAYOUT_VERSION = 'v11'

export const GLOBAL_CONFIG_YAML_FILENAME = 'config.yaml'
export const WORKSPACE_MANIFEST_FILENAME = 'pnpm-workspace.yaml'

/**
 * Named-registry aliases that work without any configuration. User entries in
 * the `namedRegistries` setting are merged on top and may override these
 * (e.g. GHES users can point `gh` at their own enterprise host).
 *
 * `npmjs` is here so a dependency can be pinned to the public registry even
 * when `registry` points somewhere else, such as an internal proxy. The `npm`
 * prefix cannot serve that purpose: it is reserved for the alias protocol
 * (`npm:<name>@<range>`), which resolves through the default registry.
 *
 * Because these URLs are also the prefixes a recorded tarball URL is matched
 * against, an org that proxies npmjs should point `npmjs` at their proxy so
 * verification keeps going there rather than to the public host.
 */
export const BUILTIN_NAMED_REGISTRIES: Readonly<Record<string, string>> = Object.freeze({
  gh: 'https://npm.pkg.github.com/',
  npmjs: 'https://registry.npmjs.org/',
})

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

// This file contains meta information
// about all the packages published by the same name, not just the manifest
// of one package/version
//
// Cache files use NDJSON format: line 1 is cache headers (etag, modified),
// line 2 is the registry metadata JSON.
export const ABBREVIATED_META_DIR = 'v11/metadata'
export const FULL_META_DIR = 'v11/metadata-full'
export const FULL_FILTERED_META_DIR = 'v11/metadata-full-filtered'
