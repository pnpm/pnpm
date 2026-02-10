import fs from 'fs/promises'
import { type PublishOptions, publish } from 'libnpmpublish'
import { type Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { type ExportedManifest } from '@pnpm/exportable-manifest'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { executeTokenHelper } from './executeTokenHelper.js'
import { createFailedToPublishError } from './FailedToPublishError.js'
import { OidcError, oidc } from './oidc.js'
import { type PackResult } from './pack.js'
import { allRegistryConfigKeys, longestRegistryConfigKey } from './registryConfigKeys.js'

type AuthConfigKey =
| 'authToken'
| 'authUserPass'
| 'tokenHelper'

type SslConfigKey =
| 'ca'
| 'cert'
| 'key'

type AuthSslConfigKey =
// default registry
| AuthConfigKey
| SslConfigKey
// other registries
| 'authInfos'
| 'sslConfigs'

export type PublishPackedPkgOptions = Pick<Config,
| AuthSslConfigKey
| 'dryRun'
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'fetchTimeout'
| 'registries'
| 'tag'
| 'userAgent'
> & {
  access?: 'public' | 'restricted'
  ci?: boolean
  otp?: string // NOTE: There is no existing test for the One-time Password feature
  provenance?: boolean
  provenanceFile?: string // NOTE: This field is currently not supported
}

// @types/libnpmpublish unfortunately uses an outdated type definition of package.json
type OutdatedManifest = typeof publish extends (_a: infer Manifest, ..._: never) => unknown ? Manifest : never

export async function publishPackedPkg (
  packResult: Pick<PackResult, 'publishedManifest' | 'tarballPath'>,
  opts: PublishPackedPkgOptions
): Promise<void> {
  const { publishedManifest, tarballPath } = packResult
  const tarballData = await fs.readFile(tarballPath)
  const publishOptions = createPublishOptions(publishedManifest, opts)
  const { name, version } = publishedManifest
  const { registry } = publishOptions
  if (registry) {
    await addAuthTokenByOidcIfApplicable(publishOptions, name, registry, opts)
  }
  globalInfo(`📦 ${name}@${version} → ${registry ?? 'the default registry'}`)
  if (opts.dryRun) {
    globalWarn(`Skip publishing ${name}@${version} (dry run)`)
    return
  }
  const response = await publish(publishedManifest as OutdatedManifest, tarballData, publishOptions)
  if (response.ok) {
    globalInfo(`✅ Published package ${name}@${version}`)
    return
  }
  throw await createFailedToPublishError(packResult, response)
}

function createPublishOptions (manifest: ExportedManifest, {
  access,
  ci: isFromCI,
  fetchRetries,
  fetchRetryFactor,
  fetchRetryMaxtimeout,
  fetchRetryMintimeout,
  fetchTimeout: timeout,
  otp,
  provenance,
  provenanceFile,
  tag: defaultTag,
  userAgent,
  ...options
}: PublishPackedPkgOptions): PublishOptions {
  const { registry, auth, ssl } = findAuthSslInfo(manifest, options)

  const publishOptions: PublishOptions = {
    access,
    defaultTag,
    fetchRetries,
    fetchRetryFactor,
    fetchRetryMaxtimeout,
    fetchRetryMintimeout,
    isFromCI,
    otp,
    timeout,
    provenance,
    provenanceFile,
    registry,
    userAgent,
    ca: ssl?.ca,
    cert: Array.isArray(ssl?.cert) ? ssl.cert.join('\n') : ssl?.cert,
    key: ssl?.key,
    token: auth && extractToken(auth),
    username: auth?.authUserPass?.username,
    password: auth?.authUserPass?.password,
  }

  pruneUndefined(publishOptions)
  return publishOptions
}

interface AuthSslInfo {
  registry: string
  auth: Pick<Config, AuthConfigKey>
  ssl: Pick<Config, SslConfigKey>
}

/**
 * Find auth and ssl information according to {@link https://docs.npmjs.com/cli/v10/configuring-npm/npmrc#auth-related-configuration}.
 *
 * The example `.npmrc` demonstrated inheritance.
 */
function findAuthSslInfo (
  { name }: ExportedManifest,
  {
    authInfos,
    sslConfigs,
    registries,
    ...defaultInfos
  }: Pick<Config, AuthSslConfigKey | 'registries'>
): Partial<AuthSslInfo> {
  // eslint-disable-next-line regexp/no-unused-capturing-group
  const scopedMatches = /@(?<scope>[^/]+)\/(?<slug>[^/]+)/.exec(name)

  const registryName = scopedMatches?.groups ? `@${scopedMatches.groups.scope}` : 'default'
  const registry = registries[registryName] ?? registries.default

  const initialRegistryConfigKey = longestRegistryConfigKey(registry)
  if (!initialRegistryConfigKey) {
    throw new PublishUnsupportedRegistryProtocolError(registry)
  }

  const result: Partial<AuthSslInfo> = { registry }

  for (const registryConfigKey of allRegistryConfigKeys(initialRegistryConfigKey)) {
    const auth: Pick<Config, AuthConfigKey> | undefined = authInfos[registryConfigKey]
    const ssl: Pick<Config, SslConfigKey> | undefined = sslConfigs[registryConfigKey]

    result.auth ??= auth // old auth from longer path collectively overrides new auth from shorter path

    result.ssl = {
      ...ssl,
      ...result.ssl, // old ssl from longer path individually overrides new ssl from shorter path
    }
  }

  if (registry !== registries.default) {
    return result
  }

  return {
    registry,
    auth: result.auth ?? defaultInfos, // old auth from longer path collectively overrides default auth
    ssl: {
      ...defaultInfos,
      ...result.ssl, // old ssl from longer path individually overrides default ssl
    },
  }
}

function extractToken ({
  token,
  tokenHelper,
}: {
  token?: string
  tokenHelper?: [string, ...string[]]
}): string | undefined {
  if (token) return token
  if (tokenHelper) {
    return executeTokenHelper(tokenHelper, { globalWarn })
  }
  return undefined
}

export class PublishUnsupportedRegistryProtocolError extends PnpmError {
  readonly registryUrl: string
  constructor (registryUrl: string) {
    super('PUBLISH_UNSUPPORTED_REGISTRY_PROTOCOL', `Registry ${registryUrl} has an unsupported protocol`, {
      hint: '`pnpm publish` only supports HTTP and HTTPS registries',
    })
    this.registryUrl = registryUrl
  }
}

/**
 * If {@link provenance} is `true`, and authentication information doesn't already set in {@link targetPublishOptions},
 * try fetching an authentication token by OpenID Connect and set it to `token` of {@link targetPublishOptions}.
 *
 * @todo Other package managers' OIDC mechanism is activated even if `--provenance` was not provided.
 *       pnpm should also active OIDC mechanism when {@link provenance} is `undefined`, and then set
 *       it to `true` when `access` was `"public"`. But this is too complex for now, so we require the
 *       user to explicitly specify `--provenance` for now.
 */
async function addAuthTokenByOidcIfApplicable (
  targetPublishOptions: PublishOptions,
  packageName: string,
  registry: string,
  options: PublishPackedPkgOptions
): Promise<void> {
  if (
    !options.provenance ||
    targetPublishOptions.token != null ||
    (targetPublishOptions.username && targetPublishOptions.password)
  ) return

  try {
    targetPublishOptions.token = await oidc({
      options,
      packageName,
      registry,
    })
  } catch (error) {
    if (error instanceof OidcError) {
      globalWarn(`Skipped OIDC: ${error.message}`)
      return
    }

    throw error
  }
}

function pruneUndefined (object: Record<string, unknown>): void {
  for (const key in object) {
    if (object[key] === undefined) {
      delete object[key]
    }
  }
}
