import { Registries } from '@pnpm/types'
import normalizeRegistryUrl from 'normalize-registry-url'
import mapValues from 'ramda/src/map'

export const DEFAULT_REGISTRIES = {
  default: 'https://registry.npmjs.org/',
}

export function normalizeRegistries (registries?: Record<string, string>): Registries {
  if (registries == null) return DEFAULT_REGISTRIES
  const normalizeRegistries = mapValues(normalizeRegistryUrl, registries)
  return {
    ...DEFAULT_REGISTRIES,
    ...normalizeRegistries,
  }
}
