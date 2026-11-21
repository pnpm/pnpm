import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import type { RegistriesByScope, RegistryConfig } from '@pnpm/types'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'

import { getPublishConfigRegistry } from '../publish/publishPackedPkg.js'
import { publishedName } from '../publishedNames.js'
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

export async function createStageContext (opts: StageOptions, packageFilter?: string): Promise<StageContext> {
  const manifest = packageFilter ? null : await safeReadProjectManifestOnly(opts.dir)
  const packageName = manifest ? publishedName(manifest) : packageFilter
  const override = opts.cliOptions?.registry
  const publishRegistry = typeof override === 'string' ? override : getPublishConfigRegistry(manifest?.publishConfig, packageName)
  const registry = getStageRegistry(opts, packageName, publishRegistry)
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

function getStageRegistry (opts: StageOptions, packageName?: string, publishRegistry?: string): string {
  const registries = getRegistries(opts)
  const registry = publishRegistry ?? (packageName
    ? pickRegistryForPackage(registries, packageName)
    : registries.default)
  return registry.endsWith('/') ? registry : `${registry}/`
}

function getRegistries (opts: StageOptions): RegistriesByScope {
  return opts.registriesByScope ?? { default: opts.registry ?? DEFAULT_REGISTRY }
}
