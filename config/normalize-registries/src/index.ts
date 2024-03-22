import '@total-typescript/ts-reset'

import mapValues from 'ramda/src/map'
import normalizeRegistryUrl from 'normalize-registry-url'

import type { Registries } from '@pnpm/types'

export const DEFAULT_REGISTRIES = {
  default: 'https://registry.npmjs.org/',
} as const

export function normalizeRegistries(
  registries?: Record<string, string> | undefined
): Registries {
  if (registries == null) {
    return DEFAULT_REGISTRIES
  }

  const normalizeRegistries = mapValues(normalizeRegistryUrl, registries)

  return {
    ...DEFAULT_REGISTRIES,
    ...normalizeRegistries,
  }
}
