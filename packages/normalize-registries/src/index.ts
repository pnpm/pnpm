import { Registries } from '@pnpm/types'
import normalizeRegistryUrl = require('normalize-registry-url')

export const DEFAULT_REGISTRIES = {
  default: 'https://registry.npmjs.org/',
}

export default function normalizeRegistries (registries?: { [scope: string]: string }): Registries {
  if (!registries) return DEFAULT_REGISTRIES

  const normalizeRegistries = {}
  for (const scope of Object.keys(registries)) {
    normalizeRegistries[scope] = normalizeRegistryUrl(registries[scope])
  }

  return {
    ...DEFAULT_REGISTRIES,
    ...normalizeRegistries,
  }
}
