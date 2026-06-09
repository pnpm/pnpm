import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import {
  getNpmSigningKeys,
  type InstalledPackageToVerify,
  type SignatureFailureCategory,
  verifyInstalledPackageSignatures,
  type VerifySignaturesOptions,
} from '@pnpm/deps.security.signatures'
import { PnpmError } from '@pnpm/error'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import { globalWarn } from '@pnpm/logger'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import type { Registries, RegistryConfig } from '@pnpm/types'
import { familySync } from 'detect-libc'

import { exePlatformPkgDirName, exePlatformPkgDirNameNext } from './installPnpm.js'

export type VerifyPnpmEngineIdentityOptions = VerifySignaturesOptions & {
  registries: Registries
  configByUri?: Record<string, RegistryConfig>
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
 * Throws when verification detects tampering (an invalid signature) or that a
 * package/version is absent from the registry. When the registry simply cannot
 * be reached (offline), it warns and returns: that is not evidence of tampering.
 * This runs only when the engine is actually being installed (a store cache
 * miss), so it does not add a network round trip to every command.
 */
export async function verifyPnpmEngineIdentity (
  envLockfile: EnvLockfile,
  pnpmVersion: string,
  opts: VerifyPnpmEngineIdentityOptions
): Promise<void> {
  const trustedKeys = getNpmSigningKeys()
  if (trustedKeys == null) return // signature verification disabled by configuration

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
    // A failure to even reach the registry is not evidence of tampering.
    globalWarn(
      `Could not verify the registry signature of pnpm@${pnpmVersion} (${String(err)}). ` +
      'Proceeding based on the lockfile integrity only.'
    )
    return
  }
  if (result.verified) return

  const tampered = result.failures.filter((f) => f.category !== 'unreachable')
  if (tampered.length > 0) {
    throw new PnpmError(
      'PNPM_ENGINE_IDENTITY_MISMATCH',
      `Refusing to run pnpm@${pnpmVersion}: its npm registry signature could not be verified ` +
      `(${describe(tampered)}). The bytes selected by this project's lockfile/registry do not match the published pnpm release.`,
      { hint: 'This can indicate a tampered lockfile or a malicious registry. Remove the `packageManager` pin or set `pmOnFail` to `ignore` if this is unexpected.' }
    )
  }

  // Only `unreachable` failures remain: don't block legitimate offline setups,
  // but make the skipped check visible.
  globalWarn(
    `Could not verify the registry signature of pnpm@${pnpmVersion} (${describe(result.failures)}). ` +
    'Proceeding based on the lockfile integrity only.'
  )
}

function collectEnginePackagesToVerify (envLockfile: EnvLockfile, registries: Registries): InstalledPackageToVerify[] {
  const pmDeps = envLockfile.importers['.']?.packageManagerDependencies ?? {}
  const toVerify: InstalledPackageToVerify[] = []

  for (const name of ['pnpm', '@pnpm/exe']) {
    const version = pmDeps[name]?.version
    if (version == null) continue
    const integrity = registryIntegrity(envLockfile.packages[`${name}@${version}`]?.resolution)
    if (integrity != null) {
      toVerify.push({ name, version, registry: pickRegistryForPackage(registries, name), integrity })
    }
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
      const integrity = registryIntegrity(envLockfile.packages[`${platformName}@${platformVersion}`]?.resolution)
      if (integrity != null) {
        toVerify.push({ name: platformName, version: platformVersion, registry: pickRegistryForPackage(registries, platformName), integrity })
      }
      break
    }
  }

  return toVerify
}

function registryIntegrity (resolution: unknown): string | undefined {
  const integrity = (resolution as { integrity?: unknown } | undefined)?.integrity
  return typeof integrity === 'string' && integrity ? integrity : undefined
}

function describe (failures: Array<{ name: string, version: string, reason: string, category: SignatureFailureCategory }>): string {
  return failures.map(({ name, version, reason }) => `${name}@${version}: ${reason}`).join('; ')
}
