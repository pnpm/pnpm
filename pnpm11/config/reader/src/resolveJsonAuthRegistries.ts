import normalizeRegistryUrl from 'normalize-registry-url'

import type { JsonAuthResult } from './loadNpmrcFiles.js'

export interface ResolvedJsonAuthRegistries {
  /** Routes from the global config file's `_auth`, which fill in only what nothing declares. */
  fallbackRegistries: Record<string, string>
  /** Routes from the `_auth` env var, which replace what the config files declare. */
  envRegistries: Record<string, string>
}

/**
 * Pick the routes `_auth` contributes to a registry map.
 *
 * An `@` credential for a registry that a config file assigns to a scope
 * authenticates that registry; it does not make it the default. When the env
 * var still offers several default candidates, a declared default among them
 * wins. Otherwise the last candidate does.
 *
 * @param declared.registries - what the config files route, keyed like `registriesByScope`
 * @param declared.defaultRegistry - the default registry a config file declared, if any
 */
export function resolveJsonAuthRegistries (
  jsonAuth: JsonAuthResult,
  declared: { registries: Record<string, string>, defaultRegistry?: string }
): ResolvedJsonAuthRegistries {
  const { default: envDefault, ...envScopedRegistries } = jsonAuth.registries
  const scopedRegistryUrls = new Set(
    Object.entries(declared.registries)
      .filter(([scope]) => scope !== 'default' && envScopedRegistries[scope] == null)
      .map(([, url]) => normalizeRegistryUrl(url))
  )
  const isScopedRegistry = (url: string): boolean => scopedRegistryUrls.has(normalizeRegistryUrl(url))

  const { default: fallbackDefault, ...fallbackRegistries } = jsonAuth.fallbackRegistries
  if (fallbackDefault != null && !isScopedRegistry(fallbackDefault)) {
    fallbackRegistries.default = fallbackDefault
  }

  const envRegistries: Record<string, string> = envScopedRegistries
  const candidates = (jsonAuth.defaultCandidates ?? (envDefault == null ? [] : [envDefault]))
    .filter((url) => !isScopedRegistry(url))
  const declaredDefault = declared.defaultRegistry == null ? undefined : normalizeRegistryUrl(declared.defaultRegistry)
  const selectedDefault = declaredDefault != null && candidates.length > 1
    ? candidates.find((url) => normalizeRegistryUrl(url) === declaredDefault)
    : candidates[candidates.length - 1]
  if (selectedDefault != null) {
    envRegistries.default = selectedDefault
  }
  return { fallbackRegistries, envRegistries }
}
