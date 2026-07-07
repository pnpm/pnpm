import crypto from 'crypto'
import fs from 'fs'
import path from 'path'
import url from 'url'
import util from 'util'

import { PnpmError } from '@pnpm/error'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions } from '@pnpm/fetch'
import { type LockfileObject } from '@pnpm/lockfile.types'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { type DepPath, type Registries } from '@pnpm/types'
import semver from 'semver'

import { NPM_SIGNING_KEYS } from './npmSigningKeys.js'

export interface RegistryKey {
  expires: string | null
  key: string
  keyid: string
  keytype: string
  scheme: string
}

/**
 * The trusted npm signing keys used to verify package-manager binaries before
 * pnpm spawns them — npm's public keys embedded in the CLI. There is
 * deliberately no way to override or disable them at runtime: a verification
 * off-switch would be a footgun, and npm mirrors work without one (they proxy
 * the same signed packument, which is verified against these keys). The keys
 * are refreshed at release time by the update-npm-signing-keys script.
 */
export function getNpmSigningKeys (): RegistryKey[] {
  return NPM_SIGNING_KEYS.map((k) => ({ ...k }))
}

export interface VerifyPnpmEngineIdentityOptions extends CreateFetchFromRegistryOptions {
  registries: Registries
  rawConfig: Record<string, string>
  retry?: { retries?: number }
  timeout?: number
  /**
   * The npm signing keys to trust. Defaults to {@link getNpmSigningKeys} (npm's
   * embedded public keys). A test seam only — passing an empty array skips
   * verification. Not reachable from project config, so it cannot be used to
   * weaken verification for a real install.
   */
  trustedKeys?: RegistryKey[]
}

interface InstalledPackageToVerify {
  name: string
  /** The registry the package was installed from — the packument (and its signatures) is fetched from here. */
  registry: string
  version: string
  /** Integrity of the bytes actually installed on disk (from the staged lockfile). */
  integrity: string
}

/**
 * Verifies that the pnpm engine staged at `stageDir` (and about to be linked
 * into the tools directory and executed) is genuinely the published `pnpm` /
 * `@pnpm/exe` — i.e. the bytes recorded in the staged lockfile carry a valid
 * npm registry signature for their exact `name@version`.
 *
 * The wanted pnpm version comes from a repository's `packageManager` field,
 * so without this check a cloned repository could make pnpm download and run
 * an arbitrary native binary. Signatures are verified against npm's embedded
 * public keys (see {@link getNpmSigningKeys}), so a registry cannot answer
 * with its own key pair; the signed packument is fetched from the configured
 * registry, which an npm mirror proxies transparently.
 *
 * Fails closed: verification failure — including an unreachable registry —
 * refuses the version switch rather than running an unverified binary. This
 * runs only when the engine is actually being installed (a tools-directory
 * cache miss), so it does not add a network round trip to every command.
 */
export async function verifyPnpmEngineIdentity (
  stageDir: string,
  targetPkgName: string,
  pnpmVersion: string,
  opts: VerifyPnpmEngineIdentityOptions
): Promise<void> {
  const trustedKeys = opts.trustedKeys ?? getNpmSigningKeys()
  if (trustedKeys.length === 0) return // test seam: no trusted keys means skip

  const lockfile = await readWantedLockfile(stageDir, { ignoreIncompatible: true })
  if (lockfile == null) {
    throw new PnpmError(
      'PNPM_ENGINE_IDENTITY_UNVERIFIABLE',
      `Cannot verify the identity of pnpm@${pnpmVersion}: the staged install has no lockfile.`
    )
  }
  const toVerify = collectEnginePackagesToVerify(lockfile, stageDir, targetPkgName, pnpmVersion, opts.registries)

  const getAuthHeader = createGetAuthHeaderByURI({ allSettings: opts.rawConfig })
  const fetchFromRegistry = createFetchFromRegistry(opts)

  const failures: EngineSignatureFailure[] = []
  await Promise.all(toVerify.map(async (pkg) => {
    const failure = await findSignatureFailure(pkg, trustedKeys, { fetchFromRegistry, getAuthHeader, retry: opts.retry, timeout: opts.timeout })
    if (failure != null) {
      failures.push({ name: pkg.name, version: pkg.version, ...failure })
    }
  }))
  if (failures.length === 0) return

  failures.sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))
  const onlyUnreachable = failures.every((f) => f.category === 'unreachable')
  throw new PnpmError(
    onlyUnreachable ? 'PNPM_ENGINE_IDENTITY_UNVERIFIABLE' : 'PNPM_ENGINE_IDENTITY_MISMATCH',
    `Refusing to run pnpm@${pnpmVersion}: its npm registry signature could not be verified ` +
    `(${failures.map(({ name, version, reason }) => `${name}@${version}: ${reason}`).join('; ')}). ` +
    'The bytes selected by this install do not match a published, signed pnpm release.',
    { hint: 'This can indicate a tampered download or a malicious/unreachable registry. Set `manage-package-manager-versions` to `false` to skip the version switch if this is unexpected.' }
  )
}

/**
 * Why a package failed signature verification:
 * - `invalid`: a registry signature is present but does not validate over the
 *   installed bytes — a strong tamper signal.
 * - `absent`: the package/version is not on the registry, or carries no
 *   signature — suspicious for a package that is expected to be signed.
 * - `unreachable`: the trust root could not be consulted (the network request
 *   failed) — typically transient/offline, not evidence of tampering.
 */
type SignatureFailureCategory = 'invalid' | 'absent' | 'unreachable'

interface EngineSignatureFailure {
  name: string
  version: string
  reason: string
  category: SignatureFailureCategory
}

function collectEnginePackagesToVerify (
  lockfile: LockfileObject,
  stageDir: string,
  targetPkgName: string,
  version: string,
  registries: Registries
): InstalledPackageToVerify[] {
  const toVerify = [engineComponentToVerify(lockfile, registries, targetPkgName, version)]
  // The bytes actually executed are the host's platform binary, listed as an
  // optional dependency of the wrapper. This applies to `@pnpm/exe` (all
  // majors) and, from v12, the `pnpm` package too (it is itself native).
  // Verify every platform package the staged install materialized on disk.
  if (targetPkgName === '@pnpm/exe' || semver.major(version) >= 12) {
    const optionalDeps = lockfile.packages?.[`${targetPkgName}@${version}` as DepPath]?.optionalDependencies ?? {}
    for (const [name, platformVersion] of Object.entries(optionalDeps)) {
      if (!fs.existsSync(path.join(stageDir, 'node_modules', name))) continue
      toVerify.push(engineComponentToVerify(lockfile, registries, name, platformVersion))
    }
  }
  return toVerify
}

function engineComponentToVerify (
  lockfile: LockfileObject,
  registries: Registries,
  name: string,
  version: string
): InstalledPackageToVerify {
  const resolution = lockfile.packages?.[`${name}@${version}` as DepPath]?.resolution
  const integrity = (resolution as { integrity?: unknown } | undefined)?.integrity
  if (typeof integrity !== 'string' || !integrity) {
    // pnpm can install a tarball without integrity, so a missing integrity must
    // fail closed rather than silently exempt that component from verification.
    throw new PnpmError(
      'PNPM_ENGINE_IDENTITY_UNVERIFIABLE',
      `Cannot verify the identity of ${name}@${version}: its integrity metadata is missing from the staged lockfile.`
    )
  }
  return { name, version, registry: pickRegistryForPackage(registries, name), integrity }
}

interface PackageSignature {
  keyid: string
  sig: string
}

interface PackumentVersion {
  dist?: {
    integrity?: string
    signatures?: unknown
    tarball?: string
  }
}

interface Packument {
  time?: Record<string, string>
  versions?: Record<string, PackumentVersion>
}

interface FetchPackumentContext {
  fetchFromRegistry: ReturnType<typeof createFetchFromRegistry>
  getAuthHeader: (uri: string) => string | undefined
  retry?: { retries?: number }
  timeout?: number
}

async function findSignatureFailure (
  pkg: InstalledPackageToVerify,
  trustedKeys: RegistryKey[],
  ctx: FetchPackumentContext
): Promise<{ reason: string, category: SignatureFailureCategory } | undefined> {
  let packument: Packument | undefined
  try {
    packument = await fetchPackument(pkg, ctx)
  } catch (err: unknown) {
    // Fetch-layer errors embed the request URL, which may carry credentials.
    return { reason: redactTextCredentials(util.types.isNativeError(err) ? err.message : String(err)), category: 'unreachable' }
  }
  if (!packument) return { reason: `${pkg.name} is not published on ${redactUrlCredentials(pkg.registry)}`, category: 'absent' }

  const version = packument.versions?.[pkg.version]
  if (!version) return { reason: `${pkg.name}@${pkg.version} was not found on ${redactUrlCredentials(pkg.registry)}`, category: 'absent' }

  const rawSignatures = version.dist?.signatures
  if (rawSignatures != null && !Array.isArray(rawSignatures)) {
    return { reason: `malformed registry signatures metadata for ${pkg.name}@${pkg.version}`, category: 'absent' }
  }
  const signatures = rawSignatures ?? []
  if (!signatures.every(isPackageSignature)) {
    return { reason: `malformed registry signatures metadata for ${pkg.name}@${pkg.version}`, category: 'absent' }
  }
  if (signatures.length === 0) {
    return { reason: `${pkg.name}@${pkg.version} has no registry signature`, category: 'absent' }
  }

  // The message is built from the installed integrity, so a signature only
  // validates when the installed bytes match what the registry signed.
  return verifyPackageSignatures(pkg, packument.time?.[pkg.version], signatures, trustedKeys)
}

async function fetchPackument (
  pkg: InstalledPackageToVerify,
  ctx: FetchPackumentContext
): Promise<Packument | undefined> {
  const registryUrl = pkg.registry.endsWith('/') ? pkg.registry : `${pkg.registry}/`
  const packumentUrl = toUri(pkg.name, registryUrl)

  const response = await ctx.fetchFromRegistry(packumentUrl, {
    authHeaderValue: ctx.getAuthHeader(registryUrl),
    fullMetadata: true,
    retry: ctx.retry,
    timeout: ctx.timeout,
  })

  if (response.status === 404) {
    return undefined
  }
  if (response.status !== 200) {
    throw new PnpmError(
      'ENGINE_IDENTITY_PACKUMENT_FETCH_FAIL',
      `The packument endpoint (at ${redactUrlCredentials(packumentUrl)}) responded with ${response.status}: ${(await response.text()).slice(0, 500)}`
    )
  }

  const body: unknown = await response.json()
  if (!isPackument(body)) {
    throw new PnpmError(
      'ENGINE_IDENTITY_PACKUMENT_FETCH_FAIL',
      `The packument endpoint (at ${redactUrlCredentials(packumentUrl)}) returned an unexpected body. Expected an object with versions; got: ${JSON.stringify(body)?.slice(0, 500) ?? String(body)}`
    )
  }
  return body
}

// Registry URLs may legally embed basic-auth credentials
// (https://user:pass@host/); never print those in error messages, which land
// in terminal output and CI logs.
function redactUrlCredentials (rawUrl: string): string {
  try {
    const parsed = new url.URL(rawUrl)
    parsed.username = ''
    parsed.password = ''
    return parsed.toString()
  } catch {
    return rawUrl
  }
}

function redactTextCredentials (text: string): string {
  return text.replace(/([a-z][a-z0-9+.-]*:\/\/)[^@/\s]+@/gi, '$1')
}

function verifyPackageSignatures (
  pkg: InstalledPackageToVerify,
  publishedAt: string | undefined,
  signatures: PackageSignature[],
  keys: RegistryKey[]
): { reason: string, category: SignatureFailureCategory } | undefined {
  // Registry signatures cover the package identity and content integrity.
  const message = `${pkg.name}@${pkg.version}:${pkg.integrity}`
  const publishedTime = publishedAt ? Date.parse(publishedAt) : undefined

  // A package is accepted as soon as ONE signature made by a trusted key
  // validates. Signatures from unknown/expired/invalid keys are recorded but do
  // not on their own fail the package — otherwise a key rotation (a packument
  // carrying multiple signatures) breaks, and a mirror could force a failure
  // just by appending a junk signature. We fail only when no signature validates
  // against a trusted key.
  const failures: string[] = []
  for (const signature of signatures) {
    const key = keys.find(({ keyid }) => keyid === signature.keyid)
    if (!key) {
      failures.push(`${pkg.name}@${pkg.version} has a registry signature with keyid ${signature.keyid} but no corresponding public key can be found`)
      continue
    }
    // Key expiry is a consistency check, not a security boundary: the publish
    // time comes from the same unauthenticated packument as the signatures, so
    // a forger holding an expired trusted key could backdate it anyway. The
    // signature verification below is what gates acceptance.
    if (key.expires && publishedTime != null && publishedTime >= Date.parse(key.expires)) {
      failures.push(`${pkg.name}@${pkg.version} has a registry signature with keyid ${signature.keyid} but the corresponding public key has expired ${key.expires}`)
      continue
    }
    const pem = `-----BEGIN PUBLIC KEY-----\n${key.key}\n-----END PUBLIC KEY-----`
    // crypto.verify can throw on malformed PEM key material or signature bytes
    // returned by the registry; treat any failure as an invalid signature so
    // one bad key doesn't crash the whole verification.
    let verified: boolean
    try {
      const verifier = crypto.createVerify('SHA256')
      verifier.write(message)
      verifier.end()
      verified = verifier.verify(pem, signature.sig, 'base64')
    } catch {
      verified = false
    }
    if (verified) return undefined
    failures.push(`${pkg.name}@${pkg.version} has an invalid registry signature with keyid ${signature.keyid}`)
  }
  // Prefer an invalid signature from a known key (a tamper signal) over an
  // unknown-key or expiry reason, since unknown keys may just be junk a mirror
  // appended.
  const reason = failures.find((failure) => failure.includes('invalid registry signature')) ??
    failures[0] ??
    `${pkg.name}@${pkg.version} has no registry signature from a trusted key`
  return { reason, category: 'invalid' }
}

function toUri (pkgName: string, registry: string): string {
  let encodedName: string
  if (pkgName[0] === '@') {
    encodedName = `@${encodeURIComponent(pkgName.slice(1))}`
  } else {
    encodedName = encodeURIComponent(pkgName)
  }
  return new url.URL(encodedName, registry.endsWith('/') ? registry : `${registry}/`).toString()
}

function isPackument (body: unknown): body is Packument {
  return typeof body === 'object' && body != null && typeof (body as Packument).versions === 'object' && (body as Packument).versions != null
}

function isPackageSignature (signature: unknown): signature is PackageSignature {
  return typeof signature === 'object' && signature != null &&
    typeof (signature as PackageSignature).keyid === 'string' &&
    typeof (signature as PackageSignature).sig === 'string'
}
