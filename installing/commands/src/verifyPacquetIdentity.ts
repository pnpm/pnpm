import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import {
  getNpmSigningKeys,
  type InstalledPackageToVerify,
  verifyInstalledPackageSignatures,
} from '@pnpm/deps.security.signatures'
import { readEnvLockfile } from '@pnpm/lockfile.fs'
import { logger } from '@pnpm/logger'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import type { CreateFetchFromRegistryOptions, RetryTimeoutOptions } from '@pnpm/network.fetch'
import type { Registries, RegistryConfig } from '@pnpm/types'

export interface VerifyPacquetIdentityOptions extends CreateFetchFromRegistryOptions {
  lockfileDir: string
  rootDir: string
  registries: Registries
  configByUri?: Record<string, RegistryConfig>
  retry?: RetryTimeoutOptions
  timeout?: number
  networkConcurrency?: number
}

/**
 * Decides whether pnpm may spawn the pacquet binary installed under
 * `node_modules/.pnpm-config/<packageName>` as an install engine.
 *
 * A repository declares pacquet in its `pnpm-workspace.yaml`
 * `configDependencies` and controls the lockfile integrity and the registry
 * the bytes came from — so the declaration alone cannot authorize running a
 * native binary. This verifies that the exact bytes installed on disk (the
 * `pacquet` shim and the host's `@pacquet/<platform>-<arch>` binary, which is
 * what actually executes) carry a valid npm registry signature for that
 * `name@version`, checked against npm's embedded public keys. The signature is
 * verified over the *installed* integrity, so substituted or tampered bytes
 * fail — and because the keys are embedded rather than fetched, a repository
 * pointing the registry at a server it controls cannot supply its own key pair.
 *
 * Returns `false` (and logs why) when identity cannot be confirmed; the caller
 * then falls back to pnpm's own install engine instead of spawning pacquet.
 */
export async function verifyPacquetIdentity (
  packageName: 'pacquet' | '@pnpm/pacquet',
  opts: VerifyPacquetIdentityOptions
): Promise<boolean> {
  const trustedKeys = getNpmSigningKeys()
  // Signature verification disabled by the machine's configuration — trust the
  // declaration and delegate.
  if (trustedKeys == null) return true

  const toVerify = await collectPacquetPackagesToVerify(packageName, opts.rootDir, opts.registries)
  if (toVerify == null) {
    return skip(packageName, opts.lockfileDir, 'its entry is missing from the lockfile')
  }

  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {})
  let result
  try {
    result = await verifyInstalledPackageSignatures(toVerify, trustedKeys, getAuthHeader, opts)
  } catch (err: unknown) {
    return skip(packageName, opts.lockfileDir, `verification could not be completed (${String(err)})`)
  }

  if (!result.verified) {
    const detail = result.failures.map(({ name, version, reason }) => `${name}@${version}: ${reason}`).join('; ')
    return skip(packageName, opts.lockfileDir, `its registry signature could not be verified (${detail})`)
  }
  return true
}

async function collectPacquetPackagesToVerify (
  packageName: string,
  rootDir: string,
  registries: Registries
): Promise<InstalledPackageToVerify[] | undefined> {
  const envLockfile = await readEnvLockfile(rootDir)
  if (envLockfile == null) return undefined

  const shim = envLockfile.importers['.']?.configDependencies?.[packageName]
  if (shim == null) return undefined
  const shimKey = `${packageName}@${shim.version}`
  const shimIntegrity = registryIntegrity(envLockfile.packages[shimKey]?.resolution)
  if (shimIntegrity == null) return undefined

  // Only the host's platform binary is ever spawned, so that's the one whose
  // identity matters. If it isn't in the lockfile, pacquet couldn't run here.
  const platformPkgName = `@pacquet/${process.platform}-${process.arch}`
  const platformVersion = envLockfile.snapshots[shimKey]?.optionalDependencies?.[platformPkgName]
  if (platformVersion == null) return undefined
  const platformKey = `${platformPkgName}@${platformVersion}`
  const platformIntegrity = registryIntegrity(envLockfile.packages[platformKey]?.resolution)
  if (platformIntegrity == null) return undefined

  return [
    { name: packageName, version: shim.version, registry: pickRegistryForPackage(registries, packageName), integrity: shimIntegrity },
    { name: platformPkgName, version: platformVersion, registry: pickRegistryForPackage(registries, platformPkgName), integrity: platformIntegrity },
  ]
}

function registryIntegrity (resolution: unknown): string | undefined {
  const integrity = (resolution as { integrity?: unknown } | undefined)?.integrity
  return typeof integrity === 'string' && integrity ? integrity : undefined
}

function skip (packageName: string, prefix: string, reason: string): false {
  logger.warn({
    message: `Not using pacquet as the install engine: ${reason}. ` +
      `Declaring "${packageName}" in configDependencies only opts in to a registry-signature-verified pacquet release; ` +
      "falling back to pnpm's default install engine.",
    prefix,
  })
  return false
}
