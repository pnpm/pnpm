import fs from 'fs/promises'
import { type PublishOptions, publish } from 'libnpmpublish'
import { type Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { type ExportedManifest } from '@pnpm/exportable-manifest'
import { globalWarn } from '@pnpm/logger'
import { executeTokenHelper } from './executeTokenHelper.js'
import { createFailedToPublishError } from './FailedToPublishError.js'
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
| 'userAgent'
> & {
  access?: 'public' | 'restricted'
  ci?: boolean
  otp?: string | number // TODO: define this config key and load this data

  // NOTE: the provenance feature requires a custom implementation of OIDC and Sigstore client, and as such, not yet available
  //       see <https://github.com/npm/cli/blob/7d900c4656cfffc8cca93240c6cda4b441fbbfaa/lib/utils/oidc.js>
  //       see <https://github.com/watson/ci-info>
  //       see <https://docs.github.com/en/actions/deployment/security-hardening-your-deployments/about-security-hardening-with-openid-connect>
  // TODO: implement provenance
  provenance?: boolean
  provenanceFile?: string
}

// @types/libnpmpublish unfortunately uses an outdated type definition of package.json
type OutdatedManifest = typeof publish extends (_a: infer Manifest, ..._: never) => unknown ? Manifest : never

export async function publishPackedPkg (packResult: PackResult, opts: PublishPackedPkgOptions): Promise<void> {
  const { publishedManifest, tarballPath } = packResult
  assertPublishPackage(publishedManifest)
  const tarballData = await fs.readFile(tarballPath)
  const publishOptions = createPublishOptions(packResult, opts)
  if (opts.dryRun) {
    globalWarn(`Skip publishing ${publishedManifest.name}@${publishedManifest.version} because of --dry-run.`)
    return
  }
  const response = await publish(publishedManifest as OutdatedManifest, tarballData, publishOptions)
  if (response.ok) return
  throw await createFailedToPublishError(packResult, response)
}

function assertPublishPackage<
  Manifest extends Pick<ExportedManifest, 'name' | 'private'>
> (manifest: Manifest): asserts manifest is Manifest & { private?: false } {
  if (manifest.private) {
    throw new PublishPrivatePackageError(manifest)
  }
}

export class PublishPrivatePackageError extends PnpmError {
  constructor ({ name }: Pick<ExportedManifest, 'name'>) {
    super('PUBLISH_PRIVATE_PACKAGE', `Cannot publish private package ${JSON.stringify(name)}`, {
      hint: 'Remove the "private" property if you intend to publish it',
    })
  }
}

function createPublishOptions (packResult: PackResult, {
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
  userAgent,
  ...options
}: PublishPackedPkgOptions): PublishOptions {
  const { registry, auth, ssl } = findAuthSslInfo(packResult.publishedManifest, options)

  const publishOptions: PublishOptions = {
    access,
    fetchRetries,
    fetchRetryFactor,
    fetchRetryMaxtimeout,
    fetchRetryMintimeout,
    isFromCI,
    otp,
    provenance,
    provenanceFile,
    timeout,
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

function pruneUndefined (object: Record<string, unknown>): void {
  for (const key in object) {
    if (object[key] === undefined) {
      delete object[key]
    }
  }
}
