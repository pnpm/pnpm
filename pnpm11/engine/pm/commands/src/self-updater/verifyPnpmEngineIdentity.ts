import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import {
  equalRegistries,
  getNpmSigningKeys,
  type InstalledPackageToVerify,
  type InstalledSignatureFailure,
  type RegistryKey,
  type SignatureFailureCategory,
  verifyInstalledPackageSignatures,
  type VerifySignaturesOptions,
} from '@pnpm/deps.security.signatures'
import { PnpmError } from '@pnpm/error'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import { globalWarn } from '@pnpm/logger'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import type { RegistriesByScope, RegistryConfig } from '@pnpm/types'
import { familySync } from 'detect-libc'
import semver from 'semver'

import { exePlatformPkgDirName, exePlatformPkgDirNameNext } from './installPnpm.js'

const CANONICAL_NPM_REGISTRY = 'https://registry.npmjs.org/'

export type VerifyPnpmEngineIdentityOptions = VerifySignaturesOptions & {
  registriesByScope: RegistriesByScope
  configByUri?: Record<string, RegistryConfig>
  /**
   * The npm signing keys to trust. Defaults to {@link getNpmSigningKeys} (npm's
   * embedded public keys). A test seam only — passing an empty array skips
   * verification. Not reachable from project config, so it cannot be used to
   * weaken verification for a real install.
   */
  trustedKeys?: RegistryKey[]
}

/**
 * Verifies that the pnpm engine about to be installed (and then executed) for an
 * automatic version switch or self-update is genuinely the published `pnpm` —
 * i.e. the bytes recorded in the env lockfile carry a valid npm registry
 * signature for their exact `name@version`.
 *
 * The wanted pnpm version comes from a repository's `packageManager` /
 * `devEngines.packageManager` field, and the project controls the lockfile
 * integrity and the registry the bytes are fetched from — so without this
 * check, a cloned repository could make pnpm download and run an arbitrary
 * native binary. Signatures are verified against npm's embedded public keys
 * (see `getNpmSigningKeys`), so a project-controlled registry cannot answer with
 * its own key pair; the signed packument is fetched from the configured registry,
 * which an npm mirror proxies transparently.
 *
 * RegistriesByScope that serve no `dist.signatures` at all (private mirrors and feed
 * proxies commonly strip them — https://github.com/pnpm/pnpm/issues/13147) do
 * not fail the check outright: the signature is then fetched from the canonical
 * npm registry instead, which proves exactly the same thing, since it is
 * verified against the embedded keys over the installed integrity. When no
 * signature can be obtained from either source (both unreachable, or the
 * integrity is a non-sha512 pin no npm signature can cover), the check warns
 * and proceeds — but only when every engine package resolves through a
 * non-canonical registry. Such a registry can only come from the user's own
 * trusted (non-project) configuration, the download URL is derived from it
 * rather than read from the lockfile, and the bytes stay pinned by the
 * lockfile integrity — so a cloned repository still cannot steer pnpm to
 * attacker-controlled bytes; the residual trust is the same the user already
 * places in that registry for every package they install from it.
 *
 * Throws when verification detects tampering (an invalid signature), when a
 * package/version is absent from a reachable canonical registry, or when an
 * engine component present in the lockfile carries no integrity metadata —
 * pnpm can install a tarball without integrity, so a missing integrity must
 * fail closed rather than silently exempt that component from verification.
 * For engine packages resolved from the canonical npm registry itself, even an
 * unreachable registry fails closed (with `PNPM_ENGINE_IDENTITY_UNVERIFIABLE`):
 * the lockfile integrity is project-controlled, so it is not a safe fallback.
 * This runs only when the engine is actually being installed (a store cache
 * miss), so it does not add a network round trip to every command.
 */
export async function verifyPnpmEngineIdentity (
  envLockfile: EnvLockfile,
  pnpmVersion: string,
  opts: VerifyPnpmEngineIdentityOptions
): Promise<void> {
  const trustedKeys = opts.trustedKeys ?? getNpmSigningKeys()
  if (trustedKeys.length === 0) return // test seam: no trusted keys means skip

  const toVerify = collectEnginePackagesToVerify(envLockfile, opts.registriesByScope)
  if (toVerify.length === 0) {
    throw new PnpmError(
      'PNPM_ENGINE_IDENTITY_UNVERIFIABLE',
      `Cannot verify the identity of pnpm@${pnpmVersion}: its integrity metadata is missing from pnpm-lock.yaml.`
    )
  }

  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {})
  let result
  try {
    result = await verifyInstalledPackageSignatures(toVerify, trustedKeys, getAuthHeader, {
      ...opts,
      fallbackRegistry: CANONICAL_NPM_REGISTRY,
    })
  } catch (err: unknown) {
    // Fail closed: we will not run a downloaded pnpm we could not verify, even
    // when the failure is "could not reach the registry". The lockfile integrity
    // is project-controlled, so it is not a safe fallback.
    throw new PnpmError(
      'PNPM_ENGINE_IDENTITY_UNVERIFIABLE',
      `Refusing to run pnpm@${pnpmVersion}: its npm registry signature could not be verified (${String(err)}).`,
      { hint: 'The registry signing keys / packument must be reachable to verify the pnpm release. Set `pmOnFail` to `ignore` to skip the version switch.' }
    )
  }
  if (result.verified) return

  if (result.failures.every(isTolerableWithoutSignature)) {
    globalWarn(
      `The authenticity of pnpm@${pnpmVersion} could not be verified against npm's registry signatures: ${describe(result.failures)}. ` +
      'Proceeding anyway, because the release was resolved through the registry configured in your own (non-project) configuration and stays pinned by its integrity checksum.'
    )
    return
  }

  const onlyUnreachable = result.failures.every((f) => f.category === 'unreachable')
  throw new PnpmError(
    onlyUnreachable ? 'PNPM_ENGINE_IDENTITY_UNVERIFIABLE' : 'PNPM_ENGINE_IDENTITY_MISMATCH',
    `Refusing to run pnpm@${pnpmVersion}: its npm registry signature could not be verified ` +
    `(${describe(result.failures)}). The bytes selected by this project's lockfile/registry do not match a published, signed pnpm release.`,
    { hint: 'This can indicate a tampered lockfile or a malicious/unreachable registry. Set `pmOnFail` to `ignore` to skip the version switch if this is unexpected.' }
  )
}

/**
 * Whether the engine may run despite `failure`: no signature was obtainable
 * (nothing suspicious was observed — as opposed to a signature that exists but
 * does not validate, or a canonical registry answering that no signed release
 * exists), and the package resolves through a registry the user configured
 * themselves. See the trust rationale on {@link verifyPnpmEngineIdentity}.
 */
function isTolerableWithoutSignature (failure: InstalledSignatureFailure): boolean {
  return (failure.category === 'unreachable' || failure.category === 'uncovered') &&
    !equalRegistries(failure.registry, CANONICAL_NPM_REGISTRY)
}

function collectEnginePackagesToVerify (envLockfile: EnvLockfile, registriesByScope: RegistriesByScope): InstalledPackageToVerify[] {
  const pmDeps = envLockfile.importers['.']?.packageManagerDependencies ?? {}
  const toVerify: InstalledPackageToVerify[] = []

  for (const name of ['pnpm', '@pnpm/exe']) {
    const version = pmDeps[name]?.version
    if (version == null) continue
    toVerify.push(engineComponentToVerify(envLockfile, registriesByScope, { name, version }))
  }

  // The bytes actually executed are the host's platform binary, listed as an
  // optional dependency of the native wrapper. Since this is the native code
  // that will run, a missing snapshot, missing optional deps, or no host
  // candidate fails closed rather than letting verification pass on the
  // wrappers alone.
  const wrapper = nativeEngineWrapper(pmDeps)
  if (wrapper != null) {
    const label = `${wrapper.name}@${wrapper.version}`
    const optionalDeps = envLockfile.snapshots[label]?.optionalDependencies
    if (optionalDeps == null) {
      throw new PnpmError(
        'PNPM_ENGINE_IDENTITY_UNVERIFIABLE',
        `Cannot verify the identity of ${label}: its platform binaries are missing from pnpm-lock.yaml.`
      )
    }
    const libcFamily = familySync()
    const candidateNames = [
      `@pnpm/${exePlatformPkgDirName(process.platform, process.arch, libcFamily)}`,
      `@pnpm/${exePlatformPkgDirNameNext(process.platform, process.arch, libcFamily)}`,
    ]
    // The first candidate present in the lockfile is the binary the install
    // will link and execute, so it is the one that must be verifiable.
    const platformName = candidateNames.find((name) => optionalDeps[name] != null)
    if (platformName == null) {
      throw new PnpmError(
        'PNPM_ENGINE_IDENTITY_UNVERIFIABLE',
        `Cannot verify the identity of the @pnpm/exe.${process.platform}-${process.arch} native binary: it is missing from pnpm-lock.yaml.`
      )
    }
    toVerify.push(engineComponentToVerify(envLockfile, registriesByScope, { name: platformName, version: optionalDeps[platformName] }))
  }

  return toVerify
}

/**
 * The engine package whose optional dependencies carry the host's native
 * binary: `@pnpm/exe` when the lockfile pins it, otherwise `pnpm` itself for
 * `>=12`, where the unscoped package is the native executable. `undefined`
 * when the lockfile pins only a JS-only `pnpm` (`<6.17.1`), which has no
 * platform binaries.
 */
function nativeEngineWrapper (pmDeps: Record<string, { version: string }>): { name: string, version: string } | undefined {
  const exeVersion = pmDeps['@pnpm/exe']?.version
  if (exeVersion != null) return { name: '@pnpm/exe', version: exeVersion }
  const pnpmVersion = pmDeps['pnpm']?.version
  if (pnpmVersion == null) return undefined
  const parsed = semver.parse(pnpmVersion, { loose: true })
  return parsed != null && parsed.major >= 12 ? { name: 'pnpm', version: pnpmVersion } : undefined
}

function engineComponentToVerify (
  envLockfile: EnvLockfile,
  registriesByScope: RegistriesByScope,
  { name, version }: { name: string, version: string }
): InstalledPackageToVerify {
  const integrity = registryIntegrity(envLockfile.packages[`${name}@${version}`]?.resolution)
  if (integrity == null) {
    throw new PnpmError(
      'PNPM_ENGINE_IDENTITY_UNVERIFIABLE',
      `Cannot verify the identity of ${name}@${version}: its integrity metadata is missing from pnpm-lock.yaml.`
    )
  }
  return { name, version, registry: pickRegistryForPackage(registriesByScope, name), integrity }
}

function registryIntegrity (resolution: unknown): string | undefined {
  const integrity = (resolution as { integrity?: unknown } | undefined)?.integrity
  return typeof integrity === 'string' && integrity ? integrity : undefined
}

function describe (failures: Array<{ name: string, version: string, reason: string, category: SignatureFailureCategory }>): string {
  return failures.map(({ name, version, reason }) => `${name}@${version}: ${reason}`).join('; ')
}
