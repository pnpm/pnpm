import fs from 'node:fs/promises'

import type { Config } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { createDispatchedFetch } from '@pnpm/network.fetch'
import type { ExportedManifest } from '@pnpm/releasing.exportable-manifest'
import { type Creds, DEFAULT_REGISTRY_SCOPE, type RegistryConfig } from '@pnpm/types'
import type { PublishOptions } from 'libnpmpublish'

import { createPublishSummary, type PublishSummary } from '../tarball/publishSummary.js'
import { displayError } from './displayError.js'
import { executeTokenHelper } from './executeTokenHelper.js'
import { createFailedToPublishError } from './FailedToPublishError.js'
import { AuthTokenError, fetchAuthToken } from './oidc/authToken.js'
import { getIdToken, IdTokenError } from './oidc/idToken.js'
import { determineProvenance, ProvenanceError } from './oidc/provenance.js'
import { type OtpContext, type PublishOptionsWithDefaultAccess, publishWithOtpHandling } from './otp.js'
import type { PackResult } from './pack.js'
import { allRegistryConfigKeys, type NormalizedRegistryUrl, parseSupportedRegistryUrl } from './registryConfigKeys.js'
import { SHARED_CONTEXT } from './utils/shared-context.js'

export type { PublishSummary }

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
> & Partial<Pick<Config,
| 'ca'
| 'cert'
| 'httpProxy'
| 'httpsProxy'
| 'key'
| 'localAddress'
| 'noProxy'
| 'strictSsl'
>> & {
  access?: 'public' | 'restricted'
  ci?: boolean
  otp?: string // NOTE: There is no existing test for the One-time Password feature
  provenance?: boolean
  provenanceFile?: string // NOTE: This field is currently not supported
  stage?: boolean
}

export async function publishPackedPkg (
  packResult: Pick<PackResult, 'publishedManifest' | 'tarballPath' | 'contents' | 'unpackedSize'>,
  opts: PublishPackedPkgOptions
): Promise<PublishSummary> {
  const { publishedManifest, tarballPath, contents, unpackedSize } = packResult
  const tarballData = await fs.readFile(tarballPath)
  const publishOptions = await createPublishOptions(publishedManifest, opts)
  const { name, version } = publishedManifest
  const { registry } = publishOptions
  const isStage = opts.stage === true
  globalInfo(`📦 ${name}@${version} → ${registry ?? 'the default registry'}`)
  const summary = createPublishSummary({ publishedManifest, tarballPath, contents, unpackedSize }, tarballData)
  if (opts.dryRun) {
    globalWarn(`Skip ${isStage ? 'staging' : 'publishing'} ${name}@${version} (dry run)`)
    return summary
  }
  const context = createPublishContext(opts)
  const response = await publishWithOtpHandling({
    context,
    manifest: publishedManifest,
    publishOptions,
    tarballData,
  })
  if (response.ok) {
    if (isStage && response.stageId) {
      summary.stageId = response.stageId
    }
    globalInfo(`✅ ${isStage ? 'Staged' : 'Published'} package ${name}@${version}`)
    return summary
  }
  throw await createFailedToPublishError(packResult, response)
}

/**
 * Builds the {@link OtpContext} used to drive the publish. The default fetch
 * is replaced by one that respects proxy / TLS / local-address settings, so
 * the `doneUrl` polling in the web-based authentication flow goes through
 * the same network configuration as the initial publish request (see
 * https://github.com/pnpm/pnpm/issues/11561).
 */
export function createPublishContext (opts: PublishPackedPkgOptions): OtpContext {
  return {
    ...SHARED_CONTEXT,
    fetch: createDispatchedFetch({ ...opts, timeout: opts.fetchTimeout }),
  }
}

type StagePublishOptions = PublishOptionsWithDefaultAccess & {
  command?: string
  stage?: boolean
}

/**
 * Build the libnpmpublish / npm-registry-fetch options (registry, auth, headers) for publishing
 * {@link manifest}. When `oidc` is `false`, the per-package OIDC token exchange is skipped and only
 * statically configured credentials are used — batch publish sends many packages in one request,
 * which a package-scoped OIDC token cannot authorize.
 *
 * @internal Exported for unit testing of the access / registry / auth fallback rules and for batch
 *   publish. Not part of the package's public API.
 */
export async function createPublishOptions (
  manifest: ExportedManifest,
  options: PublishPackedPkgOptions,
  { oidc = true }: { oidc?: boolean } = {}
): Promise<StagePublishOptions> {
  const publishConfigRegistry = typeof manifest.publishConfig?.registry === 'string'
    ? manifest.publishConfig.registry
    : undefined
  const { registry, config } = findRegistryInfo(manifest, options, publishConfigRegistry)
  const tls = config?.tls
  const creds = config?.[DEFAULT_REGISTRY_SCOPE]

  const publishConfigAccess = manifest.publishConfig?.access
  const access = options.access ?? (isPublishAccess(publishConfigAccess) ? publishConfigAccess : null)

  const {
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

  const npmCommand = options.stage === true ? 'stage' : 'publish'
  const headers: PublishOptions['headers'] = {
    'npm-auth-type': 'web',
    'npm-command': npmCommand,
  }

  const publishOptions: StagePublishOptions = {
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
    strictSSL: options.strictSsl, // npm-registry-fetch defaults to true; must be set explicitly to honour strictSsl: false
    userAgent,
    // Signal to the registry that the client supports web-based authentication.
    // Without this, the registry would never offer the web auth flow and would
    // always fall back to prompting the user for an OTP code, even when the user
    // has no OTP set up.
    authType: 'web',
    ca: tls?.ca,
    cert: tls?.cert,
    key: tls?.key,
    npmCommand,
    token: creds && extractToken(creds),
    username: creds?.basicAuth?.username,
    password: creds?.basicAuth?.password,
  }

  if (options.stage === true) {
    publishOptions.command = 'stage'
    publishOptions.stage = true
  }

  if (registry) {
    if (oidc) {
      // OIDC takes precedence over a configured static `_authToken`, mirroring the npm CLI's
      // behavior (see https://github.com/npm/cli/blob/7d900c46/lib/utils/oidc.js). Trusted
      // publishing wins whenever the registry has it configured for the package; the static
      // token is used only as a fallback when OIDC is not applicable.
      const oidcTokenProvenance = await fetchTokenAndProvenanceByOidc(manifest.name, registry, options)
      if (oidcTokenProvenance?.authToken) {
        publishOptions.token = oidcTokenProvenance.authToken
      }
      publishOptions.provenance ??= oidcTokenProvenance?.provenance
    }
    appendAuthOptionsForRegistry(publishOptions, registry)
  }

  pruneUndefined(publishOptions)
  return publishOptions
}

export function isPublishAccess (access: unknown): access is 'public' | 'restricted' {
  return access === 'public' || access === 'restricted'
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
 *
 * @internal Exported for batch publish, which groups packages by their target registry.
 */
export function findRegistryInfo (
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

  const credsScope: `@${string}` = registryName === 'default' ? DEFAULT_REGISTRY_SCOPE : registryName as `@${string}`
  let creds: Creds | undefined
  let tls: RegistryConfig['tls'] = {}
  for (const registryConfigKey of allRegistryConfigKeys(initialRegistryConfigKey)) {
    const entry = configByUri[registryConfigKey]
    if (!entry) continue
    // Auth from longer path collectively overrides shorter path
    creds ??= entry[credsScope] ?? entry[DEFAULT_REGISTRY_SCOPE]
    // TLS from longer path individually overrides shorter path
    tls = { ...entry.tls, ...tls }
  }

  const config: RegistryConfig = { tls }
  if (creds) {
    config[DEFAULT_REGISTRY_SCOPE] = creds
  }
  return {
    registry,
    config,
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
 * Try fetching an authentication token and provenance by OpenID Connect.
 *
 * The result, when defined, is intended to take precedence over any statically configured
 * authentication. This mirrors the npm CLI's OIDC flow, which always attempts the exchange
 * in supported CI environments and overwrites a configured `_authToken` on success.
 *
 * @returns the OIDC-derived authToken (and provenance flag) on success, or `undefined` when
 *   OIDC is not applicable / not configured on the registry — in which case callers should
 *   fall back to whatever static authentication they already have.
 *
 * @internal Exported for unit testing of the precedence rules. Not part of the package's
 *   public API.
 */
export async function fetchTokenAndProvenanceByOidc (
  packageName: string,
  registry: string,
  options: PublishPackedPkgOptions
): Promise<OidcTokenProvenanceResult | undefined> {
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
    // OIDC is simply not applicable here — either we're outside of CI, or we're in a CI
    // that doesn't natively drive OIDC and the user hasn't forwarded a token via
    // `NPM_ID_TOKEN`. This is the common case for local publishes, so it must stay
    // silent — only configuration *errors* in a supported CI environment surface as
    // warnings, and those come back as `IdTokenError` and are handled above.
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
      // Don't lose the OIDC-derived authToken just because we couldn't determine the
      // provenance flag — the publish itself can still go through, and that's what
      // the npm CLI does too.
      globalWarn(`Skipped setting provenance: ${displayError(error)}`)
      return { authToken }
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
function appendAuthOptionsForRegistry (targetPublishOptions: StagePublishOptions, registry: NormalizedRegistryUrl): void {
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
