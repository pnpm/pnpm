import { Registries } from '@pnpm/types'
import normalizeRegistryUrl from 'normalize-registry-url'

export const DEFAULT_REGISTRIES = {
  default: 'https://registry.npmjs.org/',
}

export function normalizeRegistries (registries?: { [scope: string]: string }): Registries {
  if (registries == null) return DEFAULT_REGISTRIES

  const normalizeRegistries = {}
  for (const scope of Object.keys(registries)) {
    normalizeRegistries[scope] = normalizeRegistryUrl(registries[scope])
  }

  return {
    ...DEFAULT_REGISTRIES,
    ...normalizeRegistries,
  }
}
