import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import type { Registries, RegistryConfig } from '@pnpm/types'

import type { StageOptions } from './types.js'

const DEFAULT_REGISTRY = 'https://registry.npmjs.org/'

/**
 * Shared per-subcommand request context. Created once at the entry of each stage
 * subcommand so that paginated calls (e.g. `stageList`) reuse a single
 * `fetchFromRegistry` instance and a precomputed auth header.
 */
export interface StageContext {
  opts: StageOptions
  registry: string
  authHeaderValue: string | undefined
  fetchFromRegistry: ReturnType<typeof createFetchFromRegistry>
}

export function createStageContext (opts: StageOptions, packageName?: string): StageContext {
  const registry = getStageRegistry(opts, packageName)
  const getAuthHeaderByUri = createGetAuthHeaderByURI(
    opts.configByUri ?? {} as Record<string, RegistryConfig>
  )
  return {
    opts,
    registry,
    authHeaderValue: packageName ? getAuthHeaderByUri(registry, { pkgName: packageName }) : getAuthHeaderByUri(registry),
    fetchFromRegistry: createFetchFromRegistry(opts),
  }
}

function getStageRegistry (opts: StageOptions, packageName?: string): string {
  const registries = getRegistries(opts)
  const registry = packageName
    ? pickRegistryForPackage(registries, packageName)
    : registries.default
  return registry.endsWith('/') ? registry : `${registry}/`
}

function getRegistries (opts: StageOptions): Registries {
  return opts.registries ?? { default: opts.registry ?? DEFAULT_REGISTRY }
}
