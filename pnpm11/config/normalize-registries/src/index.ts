import { BUILTIN_NAMED_REGISTRIES } from '@pnpm/constants'
import type {
  NamedRegistries,
  Registries,
  RegistryOptions,
  RegistryServerType,
} from '@pnpm/types'
import normalizeRegistryUrl from 'normalize-registry-url'
import { map as mapValues } from 'ramda'

export const DEFAULT_REGISTRIES: Registries = {
  default: 'https://registry.npmjs.org/',
  '@jsr': 'https://npm.jsr.io/',
}

export function normalizeRegistries (registries?: Record<string, string>): Registries {
  if (registries == null) return DEFAULT_REGISTRIES
  const normalizeRegistries = mapValues(normalizeRegistryUrl, registries)
  return {
    ...DEFAULT_REGISTRIES,
    ...normalizeRegistries,
  }
}

/**
 * User entries win on collision, so a GHES user can point `gh` at their
 * enterprise host. URLs are deliberately not normalized the way
 * `normalizeRegistries` normalizes `registries`: the name and URL are recorded
 * in lockfile dep paths, so rewriting one changes what a lockfile resolves to.
 *
 * Null-prototype because names come out of those dep paths: a crafted
 * `foo@constructor:1.0.0` must not resolve to `Object.prototype.constructor`
 * and slip past the `if (!registry)` guards that fail closed on unknown names.
 */
export function normalizeNamedRegistries (namedRegistries?: Record<string, string>): NamedRegistries {
  if (namedRegistries == null) return DEFAULT_NAMED_REGISTRIES
  return Object.assign(
    Object.create(null) as NamedRegistries,
    BUILTIN_NAMED_REGISTRIES,
    namedRegistries
  )
}

/** Shared, like `DEFAULT_REGISTRIES`, so per-package callers can cache on its identity. */
const DEFAULT_NAMED_REGISTRIES = normalizeNamedRegistries({})

/**
 * The declared layout of `registry`, or `undefined` when the user has not
 * declared one. The `undefined` case is deliberately not defaulted to `'npm'`
 * here: only registry.npmjs.org behaves that way without being told, and that
 * one default belongs with the URL builder that acts on it.
 */
export function getRegistryServerType (
  registryOptions: Record<string, RegistryOptions> | undefined,
  registry: string
): RegistryServerType | undefined {
  return registryOptions?.[normalizeRegistryUrl(registry)]?.serverType
}
