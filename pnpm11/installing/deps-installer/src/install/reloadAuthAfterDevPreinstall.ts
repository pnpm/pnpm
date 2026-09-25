import { getConfigDir, getNetworkConfigs, loadNpmrcConfig } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { reloadAuthHeaders } from '@pnpm/network.auth-header'
import type { RegistryConfig } from '@pnpm/types'

export interface DevPreinstallAuthOptions {
  configByUri?: Record<string, RegistryConfig>
  cliOptions?: Record<string, unknown>
  npmrcAuthFile?: string
  configDir?: string
  workspaceDir?: string
  dir?: string
  lockfileDir?: string
}

/**
 * `pnpm:devPreinstall` may write a new registry token into the user npmrc.
 * The install already built its auth lookups from the pre-script config, so
 * re-read the npmrc files and refresh those lookups before resolution.
 */
export function reloadAuthAfterDevPreinstall (opts: DevPreinstallAuthOptions): void {
  const configByUri = opts.configByUri
  if (configByUri == null) return
  const configDir = opts.configDir ?? getConfigDir({ env: process.env, platform: process.platform })
  const npmrc = loadNpmrcConfig({
    cliOptions: opts.cliOptions ?? {},
    configDir,
    defaultOptions: {},
    dir: opts.dir ?? opts.lockfileDir,
    moduleDirname: import.meta.dirname,
    npmrcAuthFile: opts.npmrcAuthFile,
    workspaceDir: opts.workspaceDir ?? opts.lockfileDir,
  })
  rejectProjectTokenHelper(npmrc.rawConfig, npmrc.trustedConfig)
  const fresh = getNetworkConfigs(npmrc.rawConfig).configByUri ?? {}
  for (const [uri, registryConfig] of Object.entries(fresh)) {
    configByUri[uri] = { ...configByUri[uri], ...registryConfig }
  }
  // `configByUri` is the object the store client already captured. Replacing
  // it with a copy would leave that client on the pre-script token.
  reloadAuthHeaders(configByUri, { required: true })
}

function rejectProjectTokenHelper (
  rawConfig: Record<string, unknown>,
  trustedConfig: Record<string, unknown>
): void {
  for (const [key, value] of Object.entries(rawConfig)) {
    if (!key.endsWith('tokenHelper') && key !== 'tokenHelper') continue
    if (!(key in trustedConfig) || trustedConfig[key] !== value) {
      throw new PnpmError('TOKEN_HELPER_IN_PROJECT_CONFIG',
        'tokenHelper must not be configured in project-level .npmrc',
        { hint: `The key "${key}" was found in project config. Move it to ~/.npmrc or the global pnpm auth.ini.` })
    }
  }
}
