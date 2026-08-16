import { BUILTIN_REGISTRIES_BY_PREFIX } from '@pnpm/constants'
import type { RegistriesByPrefix, RegistriesByScope, RegistryContext, RegistryServerType } from '@pnpm/types'
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
