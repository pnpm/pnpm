import fs from 'fs/promises'
import { type PublishOptions, publish } from 'libnpmpublish'
import { type Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { type ExportedManifest } from '@pnpm/exportable-manifest'
import { globalInfo, globalWarn } from '@pnpm/logger'
import enquirer from 'enquirer'
import { displayError } from './displayError.js'
import { executeTokenHelper } from './executeTokenHelper.js'
import { createFailedToPublishError } from './FailedToPublishError.js'
import { AuthTokenError, fetchAuthToken } from './oidc/authToken.js'
import { IdTokenError, getIdToken } from './oidc/idToken.js'
import { ProvenanceError, determineProvenance } from './oidc/provenance.js'
import { type PackResult } from './pack.js'
import { type NormalizedRegistryUrl, allRegistryConfigKeys, parseSupportedRegistryUrl } from './registryConfigKeys.js'

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
type ManifestFromOutdatedDefinition = typeof publish extends (_a: infer Manifest, ..._: never) => unknown ? Manifest : never

export async function publishPackedPkg (
  packResult: Pick<PackResult, 'publishedManifest' | 'tarballPath'>,
  opts: PublishPackedPkgOptions
): Promise<void> {
  const { publishedManifest, tarballPath } = packResult
  const tarballData = await fs.readFile(tarballPath)
  const publishOptions = await createPublishOptions(publishedManifest, opts)
  const { name, version } = publishedManifest
  const { registry } = publishOptions
  globalInfo(`📦 ${name}@${version} → ${registry ?? 'the default registry'}`)
  if (opts.dryRun) {
    globalWarn(`Skip publishing ${name}@${version} (dry run)`)
    return
  }
  await publishWithOtpHandling(publishedManifest as ManifestFromOutdatedDefinition, tarballData, publishOptions, packResult)
}

async function publishWithOtpHandling (
  manifest: ManifestFromOutdatedDefinition,
  tarballData: Buffer,
  publishOptions: PublishOptions,
  packResult: Pick<PackResult, 'publishedManifest'>
): Promise<void> {
  let response: Awaited<ReturnType<typeof publish>>
  try {
    response = await publish(manifest, tarballData, publishOptions)
  } catch (error) {
    if (process.stdin.isTTY && process.stdout.isTTY && isOtpError(error)) {
      let otp: string | undefined
      if (error.body?.authUrl && error.body?.doneUrl) {
        otp = await webAuthOtp(error.body.authUrl, error.body.doneUrl)
      } else {
        otp = await promptForOtp()
      }
      if (otp != null) {
        return publishWithOtpHandling(manifest, tarballData, { ...publishOptions, otp }, packResult)
      }
    }
    throw error
  }
  if (response.ok) {
    const { name, version } = packResult.publishedManifest
    globalInfo(`✅ Published package ${name}@${version}`)
    return
  }
  throw await createFailedToPublishError(packResult, response)
}

interface OtpErrorBody {
  authUrl?: string
  doneUrl?: string
}

interface OtpError {
  code: string
  body?: OtpErrorBody
}

function isOtpError (error: unknown): error is OtpError {
  return (
    error != null &&
    typeof error === 'object' &&
    'code' in error &&
    (error as Record<string, unknown>).code === 'EOTP'
  )
}

async function webAuthOtp (authUrl: string, doneUrl: string): Promise<string> {
  globalInfo(`Authenticate your account at:\n${authUrl}`)
  return pollWebAuthDone(doneUrl)
}

async function pollWebAuthDone (doneUrl: string): Promise<string> {
  const startTime = Date.now()
  const timeout = 5 * 60 * 1000 // 5 minutes

  while (true) {
    if (Date.now() - startTime > timeout) {
      throw new PnpmError('WEBAUTH_TIMEOUT', 'Web authentication timed out. Please try again.')
    }
    // eslint-disable-next-line no-await-in-loop
    await new Promise<void>(resolve => setTimeout(resolve, 1000))
    let response: Response
    try {
      // eslint-disable-next-line no-await-in-loop
      response = await fetch(doneUrl)
    } catch {
      continue
    }
    if (!response.ok) continue
    let body: { done?: boolean; token?: string }
    try {
      // eslint-disable-next-line no-await-in-loop
      body = await response.json() as { done?: boolean; token?: string }
    } catch {
      continue
    }
    if (body.done && body.token) {
      return body.token
    }
  }
}

async function promptForOtp (): Promise<string | undefined> {
  const { otp } = await enquirer.prompt<{ otp: string }>({
    message: 'This operation requires a one-time password.\nEnter OTP:',
    name: 'otp',
    type: 'input',
  })
  return otp || undefined
}

async function createPublishOptions (manifest: ExportedManifest, options: PublishPackedPkgOptions): Promise<PublishOptions> {
  const { registry, auth, ssl } = findAuthSslInfo(manifest, options)

  const {
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
  } = options

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

  // This is necessary because getNetworkConfigs initialized them as { cert: '', key: '' }
  // which may be a problem.
  // The real fix is to change the type `SslConfig` into that of partial properties, but that
  // is out of scope for now.
  removeEmptyStringProperty(publishOptions, 'cert')
  removeEmptyStringProperty(publishOptions, 'key')

  if (registry) {
    const oidcTokenProvenance = await fetchTokenAndProvenanceByOidcIfApplicable(publishOptions, manifest.name, registry, options)
    publishOptions.token ??= oidcTokenProvenance?.authToken
    publishOptions.provenance ??= oidcTokenProvenance?.provenance
    appendAuthOptionsForRegistry(publishOptions, registry)
  }

  pruneUndefined(publishOptions)
  return publishOptions
}

interface AuthSslInfo {
  registry: NormalizedRegistryUrl
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
  const nonNormalizedRegistry = registries[registryName] ?? registries.default

  const supportedRegistryInfo = parseSupportedRegistryUrl(nonNormalizedRegistry)
  if (!supportedRegistryInfo) {
    throw new PublishUnsupportedRegistryProtocolError(nonNormalizedRegistry)
  }

  const {
    normalizedUrl: registry,
    longestConfigKey: initialRegistryConfigKey,
  } = supportedRegistryInfo

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

  if (
    nonNormalizedRegistry !== registries.default &&
    registry !== registries.default &&
    registry !== parseSupportedRegistryUrl(registries.default)?.normalizedUrl
  ) {
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
  authToken,
  tokenHelper,
}: {
  authToken?: string
  tokenHelper?: [string, ...string[]]
}): string | undefined {
  if (authToken) return authToken
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

interface OidcTokenProvenanceResult {
  authToken: string
  provenance?: boolean
}

/**
 * If authentication information doesn't already set in {@link targetPublishOptions},
 * try fetching an authentication token and provenance by OpenID Connect and return it.
 */
async function fetchTokenAndProvenanceByOidcIfApplicable (
  targetPublishOptions: PublishOptions,
  packageName: string,
  registry: string,
  options: PublishPackedPkgOptions
): Promise<OidcTokenProvenanceResult | undefined> {
  if (
    targetPublishOptions.token != null ||
    (targetPublishOptions.username && targetPublishOptions.password)
  ) return undefined

  let idToken: string | undefined
  try {
    idToken = await getIdToken({
      options,
      registry,
    })
  } catch (error) {
    if (error instanceof IdTokenError) {
      globalWarn(`Skipped OIDC: ${displayError(error)}`)
      return undefined
    }

    throw error
  }
  if (!idToken) {
    globalWarn('Skipped OIDC: idToken is not available')
    return undefined
  }

  let authToken: string
  try {
    authToken = await fetchAuthToken({
      idToken,
      options,
      packageName,
      registry,
    })
  } catch (error) {
    if (error instanceof AuthTokenError) {
      globalWarn(`Skipped OIDC: ${displayError(error)}`)
      return undefined
    }

    throw error
  }

  if (options.provenance != null) {
    return {
      authToken,
      provenance: options.provenance,
    }
  }

  let provenance: boolean | undefined
  try {
    provenance = await determineProvenance({
      authToken,
      idToken,
      options,
      packageName,
      registry,
    })
  } catch (error) {
    if (error instanceof ProvenanceError) {
      globalWarn(`Skipped setting provenance: ${displayError(error)}`)
      return undefined
    }

    throw error
  }

  return { authToken, provenance }
}

/**
 * Appends authentication information to {@link targetPublishOptions} to explicitly target {@link registry}.
 *
 * `libnpmpublish` has a quirk in which it only read the authentication information from `//<registry>:_authToken`
 * instead of `token`.
 * This function fixes that by making sure the registry specific authentication information exists.
 */
function appendAuthOptionsForRegistry (targetPublishOptions: PublishOptions, registry: NormalizedRegistryUrl): void {
  const registryInfo = parseSupportedRegistryUrl(registry)
  if (!registryInfo) {
    globalWarn(`The registry ${registry} cannot be converted into a config key. Supplement is skipped. Subsequent libnpmpublish call may fail.`)
    return
  }

  const registryConfigKey = registryInfo.longestConfigKey
  targetPublishOptions[`${registryConfigKey}:_authToken`] ??= targetPublishOptions.token
  targetPublishOptions[`${registryConfigKey}:username`] ??= targetPublishOptions.username
  targetPublishOptions[`${registryConfigKey}:_password`] ??= targetPublishOptions.password && btoa(targetPublishOptions.password)
}

function removeEmptyStringProperty<Key extends string> (object: Partial<Record<Key, string>>, key: Key): void {
  if (!object[key]) {
    delete object[key]
  }
}

function pruneUndefined (object: Record<string, unknown>): void {
  for (const key in object) {
    if (object[key] === undefined) {
      delete object[key]
    }
  }
}
