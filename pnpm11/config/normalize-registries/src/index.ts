import { BUILTIN_REGISTRIES_BY_PREFIX } from '@pnpm/constants'
import { DEFAULT_REGISTRY_SCOPE, type RegistriesByPrefix, type RegistriesByScope, type RegistryContext, type RegistryDeclaration, type RegistryOptions, type RegistryServerType } from '@pnpm/types'
import normalizeRegistryUrl from 'normalize-registry-url'
import { map as mapValues } from 'ramda'

export const DEFAULT_REGISTRIES_BY_SCOPE: RegistriesByScope = {
  default: 'https://registry.npmjs.org/',
  '@jsr': 'https://npm.jsr.io/',
}

export function normalizeRegistriesByScope (registries?: Record<string, string>): RegistriesByScope {
  if (registries == null) return DEFAULT_REGISTRIES_BY_SCOPE
  const normalizeRegistriesByScope = mapValues(normalizeRegistryUrl, registries)
  return {
    ...DEFAULT_REGISTRIES_BY_SCOPE,
    ...normalizeRegistriesByScope,
  }
}

/**
 * User entries win on collision, so a GHES user can point `gh` at their
 * enterprise host. URLs are deliberately not normalized the way
 * `normalizeRegistriesByScope` normalizes `registries`: the name and URL are recorded
 * in lockfile dep paths, so rewriting one changes what a lockfile resolves to.
 *
 * Null-prototype because names come out of those dep paths: a crafted
 * `foo@constructor:1.0.0` must not resolve to `Object.prototype.constructor`
 * and slip past the `if (!registry)` guards that fail closed on unknown names.
 */
export function normalizeRegistriesByPrefix (registriesByPrefix?: Record<string, string>): RegistriesByPrefix {
  if (registriesByPrefix == null) return DEFAULT_REGISTRIES_BY_PREFIX
  return Object.assign(
    Object.create(null) as RegistriesByPrefix,
    BUILTIN_REGISTRIES_BY_PREFIX,
    registriesByPrefix
  )
}

/** Shared, like `DEFAULT_REGISTRIES_BY_SCOPE`, so per-package callers can cache on its identity. */
const DEFAULT_REGISTRIES_BY_PREFIX = normalizeRegistriesByPrefix({})

/**
 * Narrow a config-shaped object down to the registry facts alone, so the
 * install and lockfile layers are not handed the whole config, and so a
 * forwarding call site cannot drop one. The single place a new
 * {@link RegistryContext} field has to be listed.
 */
export function pickRegistryContext (source: RegistryContext): RegistryContext {
  return {
    registriesByScope: source.registriesByScope,
    registriesByPrefix: source.registriesByPrefix,
    registryOptionsByUrl: source.registryOptionsByUrl,
  }
}

/**
 * Rebuilds the declarations from the lookups they were split into, for a
 * client that has to describe its registries to a pnpr server.
 *
 * The inverse of what the config reader does to the `registries` setting,
 * minus the default registry: that one travels as the request's own
 * `registry` field.
 *
 * Entries are keyed by the URL each lookup holds rather than by a normalized
 * one, so a registry a prefix addresses without a trailing slash stays the URL
 * the client resolves against.
 *
 * A built-in route the user has not pointed elsewhere is not declared: it is
 * not part of the client's configuration, and declaring `@jsr` would put
 * npm.jsr.io in front of a pnpr server's allowlist on every request, including
 * the requests that never resolve a JSR package.
 */
export function toRegistryDeclarations (context: Partial<RegistryContext>): Record<string, RegistryDeclaration> {
  const scopeRoutes = Object.entries(context.registriesByScope ?? {}).filter(
    ([scope, registry]) => scope !== 'default' && DEFAULT_REGISTRIES_BY_SCOPE[scope] !== registry
  )
  return buildRegistryDeclarations({
    ...context,
    registriesByScope: Object.fromEntries(scopeRoutes),
  })
}

/**
 * The registries the CLI resolves from, merged across every source, in the
 * shape the `registries` setting is written in — the view `pnpm config get
 * registries` prints.
 *
 * Unlike {@link toRegistryDeclarations}, nothing is omitted: the default
 * registry is declared as the bare `@` scope, and the built-in routes — the
 * `@jsr` scope and the `npmjs` / `gh` prefixes — are declared too, because
 * this view answers "where does this install resolve from", not "what did
 * the user write". Registry keys, declaration fields, and scope lists come
 * out in one canonical order so both CLI implementations print identical
 * JSON.
 */
export function toResolvedRegistryDeclarations (context: Partial<RegistryContext>): Record<string, RegistryDeclaration> {
  return sortRegistryDeclarations(buildRegistryDeclarations({
    ...context,
    registriesByScope: { ...DEFAULT_REGISTRIES_BY_SCOPE, ...context.registriesByScope },
    registriesByPrefix: { ...BUILTIN_REGISTRIES_BY_PREFIX, ...context.registriesByPrefix },
  }))
}

/** {@link RegistryContext} with the scope map allowed to omit `default`. */
interface RegistryRoutes {
  registriesByScope?: Record<string, string>
  registriesByPrefix?: Record<string, string>
  registryOptionsByUrl?: Record<string, RegistryOptions>
}

/**
 * A registry's `prefix` field holds one name, so when several prefixes route
 * to the same URL the alphabetically last one is shown — sorted here rather
 * than left to insertion order, so both CLI implementations pick the same
 * one.
 */
function buildRegistryDeclarations (context: RegistryRoutes): Record<string, RegistryDeclaration> {
  const declarations: Record<string, RegistryDeclaration> = {}
  const declarationFor = (registry: string): RegistryDeclaration => (declarations[registry] ??= {})
  for (const [scope, registry] of Object.entries(context.registriesByScope ?? {})) {
    const declaration = declarationFor(registry)
    declaration.scopes = [...declaration.scopes ?? [], scope === 'default' ? DEFAULT_REGISTRY_SCOPE : scope]
  }
  for (const [prefix, registry] of Object.entries(context.registriesByPrefix ?? {}).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))) {
    declarationFor(registry).prefix = prefix
  }
  for (const [registry, options] of Object.entries(context.registryOptionsByUrl ?? {})) {
    if (options.serverType != null) declarationFor(registry).serverType = options.serverType
    if (options.supportsTimeField != null) declarationFor(registry).supportsTimeField = options.supportsTimeField
  }
  return declarations
}

function sortRegistryDeclarations (declarations: Record<string, RegistryDeclaration>): Record<string, RegistryDeclaration> {
  const sorted: Record<string, RegistryDeclaration> = {}
  for (const registry of Object.keys(declarations).sort()) {
    const { serverType, supportsTimeField, scopes, prefix } = declarations[registry]
    sorted[registry] = {
      ...(serverType != null ? { serverType } : {}),
      ...(supportsTimeField != null ? { supportsTimeField } : {}),
      ...(scopes != null ? { scopes: [...scopes].sort() } : {}),
      ...(prefix != null ? { prefix } : {}),
    }
  }
  return sorted
}

/**
 * The layout the user declared for `registry`, or `undefined` for none.
 *
 * Built-in layouts are deliberately not applied here — they belong with
 * `isCanonicalRegistryTarballUrl`, which acts on them, so that a code path
 * that never threads `registryOptionsByUrl` still gets them.
 */
export function getRegistryServerType (
  registryContext: Pick<RegistryContext, 'registryOptionsByUrl'>,
  registry: string
): RegistryServerType | undefined {
  return registryContext.registryOptionsByUrl?.[normalizeRegistryUrl(registry)]?.serverType
}

/**
 * Whether `registry`'s abbreviated metadata carries the `time` field, from its
 * own declaration if it has one and from the `registrySupportsTimeField`
 * setting otherwise.
 *
 * The setting is the answer for every registry a project does not describe.
 */
export function registrySupportsTimeField (
  registryContext: Pick<RegistryContext, 'registryOptionsByUrl'> & { registrySupportsTimeField?: boolean },
  registry: string
): boolean {
  return registryContext.registryOptionsByUrl?.[normalizeRegistryUrl(registry)]?.supportsTimeField
    ?? registryContext.registrySupportsTimeField
    ?? false
}
