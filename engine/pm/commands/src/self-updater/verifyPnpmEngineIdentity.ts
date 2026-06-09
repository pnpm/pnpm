import {
  type InstalledPackageToVerify,
  type SignatureFailureCategory,
  verifyInstalledPackageSignatures,
  type VerifySignaturesOptions,
} from '@pnpm/deps.security.signatures'
import { PnpmError } from '@pnpm/error'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import { globalWarn } from '@pnpm/logger'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { familySync } from 'detect-libc'

import { exePlatformPkgDirName, exePlatformPkgDirNameNext } from './installPnpm.js'

// The registry that owns the `pnpm` / `@pnpm/exe` / `@pnpm/<platform>` names and
// acts as the trust root for engine identity. Identity is verified against this
// registry, not whatever registry the project's config points at — a project can
// otherwise redirect the download to a registry it controls and sign the bytes
// with its own keys.
//
// Overridable via the `PNPM_ENGINE_IDENTITY_REGISTRY` environment variable for
// npm mirrors that proxy the canonical signing keys (and for tests). This is a
// process-level setting, not project config, so a cloned repository cannot point
// the trust root at a registry it controls.
const DEFAULT_TRUST_ROOT_REGISTRY = 'https://registry.npmjs.org/'

function trustRootRegistry (): string {
  return process.env.PNPM_ENGINE_IDENTITY_REGISTRY || DEFAULT_TRUST_ROOT_REGISTRY
}

export type VerifyPnpmEngineIdentityOptions = VerifySignaturesOptions

/**
 * Verifies, against the canonical npm registry, that the pnpm engine about to
 * be installed (and then executed) for an automatic version switch or
 * self-update is genuinely the published `pnpm` — i.e. the bytes recorded in
 * the env lockfile carry a valid npm registry signature for their exact
 * `name@version`.
 *
 * The wanted pnpm version comes from a repository's `packageManager` /
 * `devEngines.packageManager` field, and the project controls the lockfile
 * integrity and the registry the bytes are fetched from — so without this
 * check, a cloned repository could make pnpm download and run an arbitrary
 * native binary.
 *
 * Throws when verification detects tampering (an invalid signature) or that a
 * package/version is absent from the canonical registry. When the canonical
 * registry simply cannot be reached (offline, or a private mirror with no npm
 * access), it warns and returns: that is not evidence of tampering, and the
 * bytes are still pinned by the lockfile integrity. This runs only when the
 * engine is actually being installed (a store cache miss), so it does not add a
 * network round trip to every command.
 */
export async function verifyPnpmEngineIdentity (
  envLockfile: EnvLockfile,
  pnpmVersion: string,
  opts: VerifyPnpmEngineIdentityOptions
): Promise<void> {
  const registry = trustRootRegistry()
  const toVerify = collectEnginePackagesToVerify(envLockfile, registry)
  if (toVerify.length === 0) {
    throw new PnpmError(
      'PNPM_ENGINE_IDENTITY_UNVERIFIABLE',
      `Cannot verify the identity of pnpm@${pnpmVersion}: its integrity metadata is missing from pnpm-lock.yaml.`
    )
  }

  // No credentials are sent to the trust-root registry: the packument and the
  // signing keys are public, and the project's tokens must not leak here.
  const getAuthHeader = createGetAuthHeaderByURI({})
  let result
  try {
    result = await verifyInstalledPackageSignatures(toVerify, getAuthHeader, opts)
  } catch (err: unknown) {
    // A failure to even reach the trust root is not evidence of tampering.
    globalWarn(
      `Could not verify the registry signature of pnpm@${pnpmVersion} on ${registry} (${String(err)}). ` +
      'Proceeding based on the lockfile integrity only.'
    )
    return
  }
  if (result.verified) return

  const tampered = result.failures.filter((f) => f.category !== 'unreachable')
  if (tampered.length > 0) {
    throw new PnpmError(
      'PNPM_ENGINE_IDENTITY_MISMATCH',
      `Refusing to run pnpm@${pnpmVersion}: its registry signature could not be verified on ${registry} ` +
      `(${describe(tampered)}). The bytes selected by this project's lockfile/registry do not match the published pnpm release.`,
      { hint: 'This can indicate a tampered lockfile or a malicious registry. Remove the `packageManager` pin or set `pmOnFail` to `ignore` if this is unexpected.' }
    )
  }

  // Only `unreachable` failures remain: don't block legitimate offline/mirror
  // setups, but make the skipped check visible.
  globalWarn(
    `Could not verify the registry signature of pnpm@${pnpmVersion} on ${registry} (${describe(result.failures)}). ` +
    'Proceeding based on the lockfile integrity only.'
  )
}

function collectEnginePackagesToVerify (envLockfile: EnvLockfile, registry: string): InstalledPackageToVerify[] {
  const pmDeps = envLockfile.importers['.']?.packageManagerDependencies ?? {}
  const toVerify: InstalledPackageToVerify[] = []

  for (const name of ['pnpm', '@pnpm/exe']) {
    const version = pmDeps[name]?.version
    if (version == null) continue
    const integrity = registryIntegrity(envLockfile.packages[`${name}@${version}`]?.resolution)
    if (integrity != null) {
      toVerify.push({ name, version, registry, integrity })
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
        toVerify.push({ name: platformName, version: platformVersion, registry, integrity })
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
