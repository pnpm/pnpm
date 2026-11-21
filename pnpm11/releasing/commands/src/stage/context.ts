import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import type { RegistriesByScope, RegistryConfig } from '@pnpm/types'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'

import { getPublishConfigRegistry, requireSupportedRegistryUrl } from '../publish/publishPackedPkg.js'
import { parseSupportedRegistryUrl } from '../publish/registryConfigKeys.js'
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
  otpRegistryConfirmed: boolean
  authHeaderValue: string | undefined
  fetchFromRegistry: ReturnType<typeof createFetchFromRegistry>
}

export async function createStageContext (opts: StageOptions, packageFilter?: string): Promise<StageContext> {
  const override = opts.cliOptions?.registry
  const inferAuthScope = packageFilter == null && Object.values(opts.configByUri ?? {})
    .some(config => Object.keys(config).some(key => key.startsWith('@') && key !== '@'))
  const manifest = typeof override !== 'string' || inferAuthScope ? await safeReadProjectManifestOnly(opts.dir) : undefined
  const localName = manifest ? publishedName(manifest) : undefined
  const packageName = packageFilter ?? localName
  const registries = getRegistries(opts)
  const publishRegistry = packageName === localName ? getPublishConfigRegistry(manifest?.publishConfig, packageName) : undefined
  const target = typeof override === 'string' ? override : publishRegistry ?? pickRegistryForPackage(registries, packageName ?? '')
  const registry = requireSupportedRegistryUrl(target).normalizedUrl
  const getAuthHeaderByUri = createGetAuthHeaderByURI(
    opts.configByUri ?? {} as Record<string, RegistryConfig>
  )
  return {
    opts,
    registry,
    otpRegistryConfirmed: typeof override === 'string' || registry === parseSupportedRegistryUrl(registries.default)?.normalizedUrl,
    authHeaderValue: packageName ? getAuthHeaderByUri(registry, { pkgName: packageName }) : getAuthHeaderByUri(registry),
    fetchFromRegistry: createFetchFromRegistry(opts),
  }
}

function getRegistries (opts: StageOptions): RegistriesByScope {
  return opts.registriesByScope ?? { default: opts.registry ?? DEFAULT_REGISTRY }
}
