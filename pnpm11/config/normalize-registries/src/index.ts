import { BUILTIN_NAMED_REGISTRIES } from '@pnpm/constants'
import type { NamedRegistries, Registries } from '@pnpm/types'
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
 * Fill in the built-in registries so downstream code can index the result
 * directly instead of re-merging them at every lookup.
 *
 * The user's entries win on collision, which is what lets a GHES user point
 * `gh` at their enterprise host, or an org that mirrors npmjs point `npmjs`
 * at their mirror.
 *
 * URLs are not normalized here the way `normalizeRegistries` normalizes
 * `registries`: the name is recorded in the lockfile's dep paths, and the URL
 * it maps to is compared against recorded tarball URLs, so rewriting it would
 * change what an existing lockfile resolves to.
 */
export function normalizeNamedRegistries (namedRegistries?: Record<string, string>): NamedRegistries {
  if (namedRegistries == null) return DEFAULT_NAMED_REGISTRIES
  return {
    ...BUILTIN_NAMED_REGISTRIES,
    ...namedRegistries,
  } as NamedRegistries
}

/**
 * The result for a project that configures no registries of its own.
 *
 * Shared rather than rebuilt per call, mirroring `DEFAULT_REGISTRIES`: callers
 * in per-package loops key a cache on this object's identity, so handing back
 * a fresh copy each time would defeat it on the common path.
 */
const DEFAULT_NAMED_REGISTRIES = { ...BUILTIN_NAMED_REGISTRIES } as NamedRegistries
