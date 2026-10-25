import { pnpmRegistryConfig } from './pnpmConfig.js'
import { DEFAULT_REGISTRY, type RequestHeaders } from './registry.js'
import type { SigningKey } from './verifySignature.js'

export interface CorepackDownloadOptions {
  registry: string
  headers?: RequestHeaders
  ca?: string[]
  keys?: SigningKey[]
  verifySignature?: false
}

/**
 * The options for `downloadPnpmExecutable` that download pnpm where
 * Corepack would, verified the way Corepack verifies it.
 *
 * - `COREPACK_NPM_REGISTRY` names the registry. Otherwise `pnpmRegistryConfig`
 *   reads pnpm's config.
 * - `COREPACK_NPM_TOKEN`, or `COREPACK_NPM_USERNAME` with
 *   `COREPACK_NPM_PASSWORD`, are the credentials for Corepack's registry, and
 *   for the npm registry when pnpm's config holds none for it.
 * - `COREPACK_INTEGRITY_KEYS` follows Corepack. Unset, npm's own keys are
 *   trusted. A key set trusts its `npm` keys. `0` or empty skips the signature
 *   check, which a registry that re-publishes packages without npm's
 *   signatures already needs for Corepack to have installed pnpm from it.
 * - The project `.npmrc` is read only while the signature is checked; see
 *   `PnpmRegistryConfigOptions.readProjectNpmrc`.
 *
 * @throws if `COREPACK_ENABLE_NETWORK` is `0`, or `COREPACK_INTEGRITY_KEYS`
 * holds no `npm` key set.
 */
export function corepackDownloadOptions (
  opts: { cwd?: string, env?: NodeJS.ProcessEnv, homeDir?: string } = {}
): CorepackDownloadOptions {
  const env = opts.env ?? process.env
  if (env.COREPACK_ENABLE_NETWORK === '0') {
    throw new Error('Network access is disabled by the environment, so the pnpm binary cannot be downloaded.')
  }
  const signature = signaturePolicy(env.COREPACK_INTEGRITY_KEYS)
  return { ...registryAccess({ ...opts, env, readProjectNpmrc: signature.verifySignature !== false }), ...signature }
}

function registryAccess (
  opts: { cwd?: string, env: NodeJS.ProcessEnv, homeDir?: string, readProjectNpmrc: boolean }
): Pick<CorepackDownloadOptions, 'registry' | 'headers' | 'ca'> {
  const corepackRegistry = opts.env.COREPACK_NPM_REGISTRY
  if (corepackRegistry) {
    return withHeaders({ registry: corepackRegistry }, corepackHeaders(opts.env))
  }
  const config = pnpmRegistryConfig(opts)
  if (config.headers == null && new URL(config.registry).origin === new URL(DEFAULT_REGISTRY).origin) {
    return withHeaders(config, corepackHeaders(opts.env))
  }
  return config
}

function withHeaders<Config extends object> (config: Config, headers: RequestHeaders | undefined): Config & { headers?: RequestHeaders } {
  return headers == null ? config : { ...config, headers }
}

function corepackHeaders (env: NodeJS.ProcessEnv): RequestHeaders | undefined {
  const { COREPACK_NPM_TOKEN, COREPACK_NPM_USERNAME, COREPACK_NPM_PASSWORD } = env
  if (COREPACK_NPM_TOKEN) {
    return { authorization: `Bearer ${COREPACK_NPM_TOKEN}` }
  }
  if (COREPACK_NPM_USERNAME && COREPACK_NPM_PASSWORD) {
    return { authorization: `Basic ${Buffer.from(`${COREPACK_NPM_USERNAME}:${COREPACK_NPM_PASSWORD}`).toString('base64')}` }
  }
  return undefined
}

function signaturePolicy (configured: string | undefined): Pick<CorepackDownloadOptions, 'keys' | 'verifySignature'> {
  if (configured == null) return {}
  if (configured === '' || configured === '0') return { verifySignature: false }
  let keys: unknown
  try {
    keys = (JSON.parse(configured) as { npm?: unknown } | null)?.npm
  } catch (err) {
    throw new Error(`COREPACK_INTEGRITY_KEYS is not readable as JSON: ${(err as Error).message}`, { cause: err })
  }
  // Passing no keys would fall back to npm's own, the opposite of what setting
  // the variable asked for. An empty set names no key to trust, and Corepack
  // reads it the same way, so every download is refused.
  if (!Array.isArray(keys)) {
    throw new Error('COREPACK_INTEGRITY_KEYS holds no "npm" key set to verify the pnpm binary against.')
  }
  return { keys: keys as SigningKey[] }
}
