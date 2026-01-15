import fs from 'fs/promises'
import { type PublishOptions, publish } from 'libnpmpublish'
import { type Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { type ExportedManifest } from '@pnpm/exportable-manifest'
import { FailedToPublishError } from './FailedToPublishError.js'
import { type PackResult } from './pack.js'
import { allRegistryConfigKeys, longestRegistryConfigKey } from './registryConfigKeys.js'

export type Options = Pick<Config,
| 'registries'
| 'sslConfigs'
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
  registries,
  sslConfigs,
  userAgent,
}: Options): PublishOptions {
  const authInfo = findAuthInfo(packResult.publishedManifest, { registries, sslConfigs })

  const publishOptions: PublishOptions = {
    ...authInfo,
    access,
    userAgent,
  }

  pruneUndefined(publishOptions)
  return publishOptions
}

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

function findAuthInfo ({ name }: ExportedManifest, { registries, sslConfigs }: Pick<Config, 'registries' | 'sslConfigs'>): AuthInfo {
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

  return { registry }
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
