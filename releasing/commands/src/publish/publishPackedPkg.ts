import { createHash } from 'node:crypto'
import fs from 'node:fs/promises'
import path from 'node:path'

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

/**
 * Per-package summary describing a successful publish, modeled after `npm publish --json`.
 * Returned to callers and serialized to stdout when `pnpm publish --json` is used.
 */
export interface PublishSummary {
  /** Human-readable identifier `${name}@${version}`. */
  id: string
  name: string
  version: string
  /** Compressed tarball size in bytes. */
  size: number
  /** Total uncompressed size of all files in the tarball, in bytes. */
  unpackedSize: number
  /** Lowercase hex SHA-1 digest of the tarball. */
  shasum: string
  /** SRI-formatted SHA-512 digest of the tarball (e.g. `sha512-...`). */
  integrity: string
  /** Tarball file basename (e.g. `pkg-1.0.0.tgz`). */
  filename: string
  /** Files inside the tarball, in the same shape `pnpm pack --json` emits. */
  files: Array<{ path: string }>
  /** Number of files inside the tarball. */
  entryCount: number
  /** Names of bundled dependencies included in the tarball (typically empty). */
  bundled: string[]
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
  globalInfo(`📦 ${name}@${version} → ${registry ?? 'the default registry'}`)
  const summary: PublishSummary = {
    id: `${name}@${version}`,
    name: name as string,
    version: version as string,
    size: tarballData.byteLength,
    unpackedSize,
    // SHA-1 is what `npm publish --json` reports as `shasum` for back-compat with the registry's
    // legacy dist.shasum field; `integrity` below is the modern SRI hash.
    shasum: createHash('sha1').update(tarballData).digest('hex'),
    integrity: `sha512-${createHash('sha512').update(tarballData).digest('base64')}`,
    filename: path.basename(tarballPath),
    files: contents.map((file) => ({ path: file })),
    entryCount: contents.length,
    bundled: extractBundledDependencies(publishedManifest),
  }
  if (opts.dryRun) {
    globalWarn(`Skip publishing ${name}@${version} (dry run)`)
    return summary
  }
  const response = await publishWithOtpHandling({ manifest: publishedManifest, tarballData, publishOptions })
  if (response.ok) {
    globalInfo(`✅ Published package ${name}@${version}`)
    return summary
  }
  throw await createFailedToPublishError(packResult, response)
}

/**
 * npm accepts both `bundledDependencies` and `bundleDependencies` in package.json and normalizes
 * to a list of dependency names. We mirror that normalization so consumers see a consistent array.
 */
function extractBundledDependencies (manifest: ExportedManifest): string[] {
  const raw = manifest.bundledDependencies ?? manifest.bundleDependencies
  if (!raw) return []
  if (Array.isArray(raw)) return raw
  // `true` means "bundle every dependency" per npm's semantics; expand it to the dependency names.
  if (raw === true) return Object.keys(manifest.dependencies ?? {})
  return []
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
    // OIDC takes precedence over a configured static `_authToken`, mirroring the npm CLI's
    // behavior (see https://github.com/npm/cli/blob/7d900c46/lib/utils/oidc.js). Trusted
    // publishing wins whenever the registry has it configured for the package; the static
    // token is used only as a fallback when OIDC is not applicable.
    const oidcTokenProvenance = await fetchTokenAndProvenanceByOidc(manifest.name, registry, options)
    if (oidcTokenProvenance?.authToken) {
      publishOptions.token = oidcTokenProvenance.authToken
    }
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
