import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import {
  getNpmSigningKeys,
  type InstalledPackageToVerify,
  type RegistryKey,
  type SignatureFailureCategory,
  verifyInstalledPackageSignatures,
  type VerifySignaturesOptions,
} from '@pnpm/deps.security.signatures'
import { PnpmError } from '@pnpm/error'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import type { Registries, RegistryConfig } from '@pnpm/types'
import { familySync } from 'detect-libc'

import { exePlatformPkgDirName, exePlatformPkgDirNameNext } from './installPnpm.js'

export type VerifyPnpmEngineIdentityOptions = VerifySignaturesOptions & {
  registries: Registries
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
 * Throws when verification detects tampering (an invalid signature), when a
 * package/version is absent from the registry, or when an engine component
 * present in the lockfile carries no integrity metadata — pnpm can install a
 * tarball without integrity, so a missing integrity must fail closed rather
 * than silently exempt that component from verification. Even an unreachable
 * registry fails closed (with `PNPM_ENGINE_IDENTITY_UNVERIFIABLE`): the
 * lockfile integrity is project-controlled, so it is not a safe fallback.
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

  const toVerify = collectEnginePackagesToVerify(envLockfile, opts.registries)
  if (toVerify.length === 0) {
    throw new PnpmError(
      'PNPM_ENGINE_IDENTITY_UNVERIFIABLE',
      `Cannot verify the identity of pnpm@${pnpmVersion}: its integrity metadata is missing from pnpm-lock.yaml.`
    )
  }

  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {})
  let result
  try {
    result = await verifyInstalledPackageSignatures(toVerify, trustedKeys, getAuthHeader, opts)
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

  const onlyUnreachable = result.failures.every((f) => f.category === 'unreachable')
  throw new PnpmError(
    onlyUnreachable ? 'PNPM_ENGINE_IDENTITY_UNVERIFIABLE' : 'PNPM_ENGINE_IDENTITY_MISMATCH',
    `Refusing to run pnpm@${pnpmVersion}: its npm registry signature could not be verified ` +
    `(${describe(result.failures)}). The bytes selected by this project's lockfile/registry do not match a published, signed pnpm release.`,
    { hint: 'This can indicate a tampered lockfile or a malicious/unreachable registry. Set `pmOnFail` to `ignore` to skip the version switch if this is unexpected.' }
  )
}

function collectEnginePackagesToVerify (envLockfile: EnvLockfile, registries: Registries): InstalledPackageToVerify[] {
  const pmDeps = envLockfile.importers['.']?.packageManagerDependencies ?? {}
  const toVerify: InstalledPackageToVerify[] = []

  for (const name of ['pnpm', '@pnpm/exe']) {
    const version = pmDeps[name]?.version
    if (version == null) continue
    toVerify.push(engineComponentToVerify(envLockfile, registries, { name, version }))
  }

  // The bytes actually executed are the host's `@pnpm/exe` platform binary,
  // listed as an optional dependency of `@pnpm/exe`.
  const exeVersion = pmDeps['@pnpm/exe']?.version
  if (exeVersion != null) {
    const optionalDeps = envLockfile.snapshots[`@pnpm/exe@${exeVersion}`]?.optionalDependencies ?? {}
    const libcFamily = familySync()
    const candidateNames = [
      `@pnpm/${exePlatformPkgDirName(process.platform, process.arch, libcFamily)}`,
      `@pnpm/${exePlatformPkgDirNameNext(process.platform, process.arch, libcFamily)}`,
    ]
    for (const platformName of candidateNames) {
      const platformVersion = optionalDeps[platformName]
      if (platformVersion == null) continue
      // The first candidate present in the lockfile is the binary the install
      // will link and execute, so it is the one that must be verifiable.
      toVerify.push(engineComponentToVerify(envLockfile, registries, { name: platformName, version: platformVersion }))
      break
    }
  }

  return toVerify
}

function engineComponentToVerify (
  envLockfile: EnvLockfile,
  registries: Registries,
  { name, version }: { name: string, version: string }
): InstalledPackageToVerify {
  const integrity = registryIntegrity(envLockfile.packages[`${name}@${version}`]?.resolution)
  if (integrity == null) {
    throw new PnpmError(
      'PNPM_ENGINE_IDENTITY_UNVERIFIABLE',
      `Cannot verify the identity of ${name}@${version}: its integrity metadata is missing from pnpm-lock.yaml.`
    )
  }
  return { name, version, registry: pickRegistryForPackage(registries, name), integrity }
}

function registryIntegrity (resolution: unknown): string | undefined {
  const integrity = (resolution as { integrity?: unknown } | undefined)?.integrity
  return typeof integrity === 'string' && integrity ? integrity : undefined
}

function describe (failures: Array<{ name: string, version: string, reason: string, category: SignatureFailureCategory }>): string {
  return failures.map(({ name, version, reason }) => `${name}@${version}: ${reason}`).join('; ')
}
