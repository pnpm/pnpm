import fs from 'node:fs/promises'

import type { Config } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { globalInfo, globalWarn } from '@pnpm/logger'
import type { ExportedManifest } from '@pnpm/releasing.exportable-manifest'
import type { Creds, RegistryConfig } from '@pnpm/types'
import type { PublishOptions } from 'libnpmpublish'

import { displayError } from './displayError.js'
import { executeTokenHelper } from './executeTokenHelper.js'
import { createFailedToPublishError } from './FailedToPublishError.js'
import { AuthTokenError, fetchAuthToken } from './oidc/authToken.js'
import { getIdToken, IdTokenError } from './oidc/idToken.js'
import { determineProvenance, ProvenanceError } from './oidc/provenance.js'
import { publishWithOtpHandling } from './otp.js'
import type { PackResult } from './pack.js'
import { allRegistryConfigKeys, type NormalizedRegistryUrl, parseSupportedRegistryUrl } from './registryConfigKeys.js'

export type PublishPackedPkgOptions = Pick<Config,
| 'configByUri'
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
  const response = await publishWithOtpHandling({ manifest: publishedManifest, tarballData, publishOptions })
  if (response.ok) {
    globalInfo(`✅ Published package ${name}@${version}`)
    return
  }
  throw await createFailedToPublishError(packResult, response)
}

async function createPublishOptions (manifest: ExportedManifest, options: PublishPackedPkgOptions): Promise<PublishOptions> {
  const publishConfigRegistry = typeof manifest.publishConfig?.registry === 'string'
    ? manifest.publishConfig.registry
    : undefined
  const { registry, config } = findRegistryInfo(manifest, options, publishConfigRegistry)
  const { creds, tls } = config ?? {}

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

  const headers: PublishOptions['headers'] = {
    'npm-auth-type': 'web',
    'npm-command': 'publish',
  }

  const publishOptions: PublishOptions = {
    access,
    defaultTag,
    fetchRetries,
    fetchRetryFactor,
    fetchRetryMaxtimeout,
    fetchRetryMintimeout,
    headers,
    isFromCI,
    otp,
    timeout,
    provenance,
    provenanceFile,
    registry,
    userAgent,
    // Signal to the registry that the client supports web-based authentication.
    // Without this, the registry would never offer the web auth flow and would
    // always fall back to prompting the user for an OTP code, even when the user
    // has no OTP set up.
    authType: 'web',
    ca: tls?.ca,
    cert: tls?.cert,
    key: tls?.key,
    npmCommand: 'publish',
    token: creds && extractToken(creds),
    username: creds?.basicAuth?.username,
    password: creds?.basicAuth?.password,
  }

  if (registry) {
    const oidcTokenProvenance = await fetchTokenAndProvenanceByOidcIfApplicable(publishOptions, manifest.name, registry, options)
    publishOptions.token ??= oidcTokenProvenance?.authToken
    publishOptions.provenance ??= oidcTokenProvenance?.provenance
    appendAuthOptionsForRegistry(publishOptions, registry)
  }

  pruneUndefined(publishOptions)
  return publishOptions
}

interface RegistryInfo {
  registry: NormalizedRegistryUrl
  config: RegistryConfig
}

/**
 * Find credentials and SSL info for a package's registry.
 * Follows {@link https://docs.npmjs.com/cli/v10/configuring-npm/npmrc#auth-related-configuration}.
 *
 * The manifest's `publishConfig.registry`, when set, takes precedence over `registries`.
 */
function findRegistryInfo (
  { name }: ExportedManifest,
  { configByUri, registries }: Pick<Config, 'configByUri' | 'registries'>,
  publishConfigRegistry?: string
): Partial<RegistryInfo> {
  // eslint-disable-next-line regexp/no-unused-capturing-group
  const scopedMatches = /@(?<scope>[^/]+)\/(?<slug>[^/]+)/.exec(name)

  const registryName = scopedMatches?.groups ? `@${scopedMatches.groups.scope}` : 'default'
  const nonNormalizedRegistry = publishConfigRegistry ?? registries[registryName] ?? registries.default

  const supportedRegistryInfo = parseSupportedRegistryUrl(nonNormalizedRegistry)
  if (!supportedRegistryInfo) {
    throw new PublishUnsupportedRegistryProtocolError(nonNormalizedRegistry)
  }

  const {
    normalizedUrl: registry,
    longestConfigKey: initialRegistryConfigKey,
  } = supportedRegistryInfo

  let creds: Creds | undefined
  let tls: RegistryConfig['tls'] = {}
  for (const registryConfigKey of allRegistryConfigKeys(initialRegistryConfigKey)) {
    const entry = configByUri[registryConfigKey]
    if (!entry) continue
    // Auth from longer path collectively overrides shorter path
    creds ??= entry.creds
    // TLS from longer path individually overrides shorter path
    tls = { ...entry.tls, ...tls }
  }

  const isDefaultRegistry =
    nonNormalizedRegistry === registries.default ||
    registry === registries.default ||
    registry === parseSupportedRegistryUrl(registries.default)?.normalizedUrl

  if (isDefaultRegistry) {
    creds ??= configByUri['']?.creds
  }

  return {
    registry,
    config: { creds, tls },
  }
}

function extractToken ({
  authToken,
  tokenHelper,
}: Pick<Creds, 'authToken' | 'tokenHelper'>): string | undefined {
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

function pruneUndefined (object: Record<string, unknown>): void {
  for (const key in object) {
    if (object[key] === undefined) {
      delete object[key]
    }
  }
}
