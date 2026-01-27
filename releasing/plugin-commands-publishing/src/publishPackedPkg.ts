import fs from 'fs/promises'
import { type PublishOptions, publish } from 'libnpmpublish'
import { type Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { type ExportedManifest } from '@pnpm/exportable-manifest'
import { FailedToPublishError } from './FailedToPublishError.js'
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

export type Options = Pick<Config,
| AuthSslConfigKey
| 'registries'
| 'userAgent'
> & {
  access?: 'public' | 'restricted'
}

// @types/libnpmpublish unfortunately uses an outdated type definition of package.json
type OutdatedManifest = typeof publish extends (_a: infer Manifest, ..._: never) => unknown ? Manifest : never

export async function publishPackedPkg (packResult: PackResult, opts: Options): Promise<void> {
  const { publishedManifest, tarballPath } = packResult
  const tarballData = await fs.readFile(tarballPath)
  const response = await publish(publishedManifest as OutdatedManifest, tarballData, createPublishOptions(packResult, opts))
  if (response.ok) return
  throw await FailedToPublishError.createFailedToPublishError(packResult, response)
}

function createPublishOptions (packResult: PackResult, {
  access,
  userAgent,
  ...options
}: Options): PublishOptions {
  const authInfo = findAuthInfo(packResult.publishedManifest, options)

  const publishOptions: PublishOptions = {
    ...authInfo,
    access,
    userAgent,
  }

  pruneUndefined(publishOptions)
  return publishOptions
}

// TODO: rename AuthInfo and findAuthInfo to appropriate names
type AuthInfo = Pick<PublishOptions,
// auth by login
| 'username' // TODO: get from first half of _auth
| 'password' // TODO: get from second half of _auth
// auth by token
| 'token' // TODO: get from _authToken
// network
| 'registry'
| 'ca'
| 'cert'
| 'key'
>

// TODO: perhaps running a single findAuthInfo in a single pass was not the way?
//       perhaps it is better to split this function into multiple, each finding their own properties?
function findAuthInfo (
  { name }: ExportedManifest,
  {
    authInfos,
    sslConfigs,
    registries,
    ...defaultInfos
  }: Pick<Config, AuthSslConfigKey | 'registries'>
): AuthInfo {
  // eslint-disable-next-line regexp/no-unused-capturing-group
  const scopedMatches = /@(?<scope>[^/]+)\/(?<slug>[^/]+)/.exec(name)

  const registryName = scopedMatches?.groups ? `@${scopedMatches.groups.scope}` : 'default'
  const registry = registries[registryName] ?? registries.default

  const initialRegistryConfigKey = longestRegistryConfigKey(registry)
  if (!initialRegistryConfigKey) {
    throw new PublishUnsupportedRegistryProtocolError(registry)
  }

  for (const registryConfigKey of allRegistryConfigKeys(initialRegistryConfigKey)) {
    const ssl: typeof sslConfigs[string] | undefined = sslConfigs[registryConfigKey]

    if (ssl) {
      // TODO: _auth
      // TODO: _authToken
      return { ...ssl, registry }
    }
  }

  return {
    registry,
    ca: defaultInfos.ca,
    cert: Array.isArray(defaultInfos.cert) ? defaultInfos.cert[0] : defaultInfos.cert, // TODO: when cert could possibly be an array?
    key: defaultInfos.key,
    token: extractToken(defaultInfos),
    username: defaultInfos.authUserPass?.username,
    password: defaultInfos.authUserPass?.password,
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
    throw new Error('TODO: execute tokenHelper')
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
