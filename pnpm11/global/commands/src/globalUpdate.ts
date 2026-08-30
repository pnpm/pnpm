import fs from 'node:fs'
import path from 'node:path'

import type { CommandHandlerMap } from '@pnpm/cli.command'
import { summaryLogger } from '@pnpm/core-loggers'
import {
  cleanOrphanedInstallDirs,
  createInstallDir,
  getGlobalPackageDetails,
  getHashLink,
  type GlobalPackageInfo,
  scanGlobalPackages,
} from '@pnpm/global.packages'
import type { CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import semver from 'semver'

import { getBinNamesOfOtherGroups } from './binOwnership.js'
import { checkGlobalBinConflicts } from './checkGlobalBinConflicts.js'
import { activateGlobalInstall, cleanupReplacedGlobalInstalls } from './globalActivation.js'
import {
  installGlobalPackages,
  type InstallGlobalPackagesResult,
  type ResolutionPolicyViolation,
} from './installGlobalPackages.js'
import { hasPnpmCliDependency } from './pnpmCliPackages.js'
import { promptApproveGlobalBuilds } from './promptApproveGlobalBuilds.js'
import { readInstalledPackages } from './readInstalledPackages.js'

export type GlobalUpdateOptions = CreateStoreControllerOptions & {
  bin?: string
  globalPkgDir?: string
  latest?: boolean
  allowBuilds?: Record<string, string | boolean>
  saveExact?: boolean
  savePrefix?: string
  rootProjectManifest?: unknown
  handleResolutionPolicyViolations?: (violations: readonly ResolutionPolicyViolation[]) => Promise<void>
  updateResolutionPolicyManifest?: (violations: readonly ResolutionPolicyViolation[], dir: string) => Promise<void>
  selectedPackageHashes?: Set<string>
}

export async function handleGlobalUpdate (
  opts: GlobalUpdateOptions,
  params: string[],
  commands: CommandHandlerMap
): Promise<string | undefined> {
  const globalDir = opts.globalPkgDir!
  const globalBinDir = opts.bin!
  cleanOrphanedInstallDirs(globalDir)
  const scannedPackages = scanGlobalPackages(globalDir)

  if (scannedPackages.length === 0) {
    return 'No global packages found'
  }
  const allPackages = scannedPackages.filter((pkg) => !hasPnpmCliDependency(pkg))
  if (allPackages.length === 0) {
    return 'No global packages to update. Run "pnpm self-update" to update pnpm itself.'
  }

  // If specific packages are requested, filter to only groups containing them
  let packagesToUpdate: GlobalPackageInfo[]
  if (params.length > 0) {
    packagesToUpdate = allPackages.filter((pkg) =>
      params.some((p) => Object.hasOwn(pkg.dependencies, p))
    )
    if (packagesToUpdate.length === 0) {
      return 'No matching global packages found'
    }
  } else {
    packagesToUpdate = allPackages
  }
  const selectedPackageHashes = opts.selectedPackageHashes
  if (selectedPackageHashes) {
    packagesToUpdate = packagesToUpdate.filter(({ hash }) => selectedPackageHashes.has(hash))
  }

  // Update each package group sequentially to avoid overwhelming the system

  for (const pkg of packagesToUpdate) {
    await updateGlobalPackageGroup(opts, globalDir, globalBinDir, pkg, commands) // eslint-disable-line no-await-in-loop
  }
  summaryLogger.debug({ prefix: globalDir })
  return undefined
}

async function updateGlobalPackageGroup (
  opts: GlobalUpdateOptions,
  globalDir: string,
  globalBinDir: string,
  pkg: GlobalPackageInfo,
  commands: CommandHandlerMap
): Promise<void> {
  const installDir = createInstallDir(globalDir)
  const pins = await pinsForDowngrades(opts, installDir, pkg)
  const { ignoredBuilds, resolutionPolicyViolations } =
    await installGroup(opts, installDir, depSpecsForUpdate(pkg.dependencies, opts.latest, pins))

  await promptApproveGlobalBuilds({
    globalPkgDir: globalDir,
    installDir,
    ignoredBuilds,
    allowBuilds: opts.allowBuilds ?? {},
    inheritedOpts: opts,
  }, commands)

  // Check for bin name conflicts with other global packages
  const pkgs = await readInstalledPackages(installDir)
  let binsToSkip: Set<string>
  try {
    binsToSkip = await checkGlobalBinConflicts({
      globalDir,
      globalBinDir,
      newPkgs: pkgs,
      shouldSkip: (existingPkg) => existingPkg.hash === pkg.hash,
    })
  } catch (err) {
    await fs.promises.rm(installDir, { recursive: true, force: true })
    throw err
  }

  const protectedBins = await getBinNamesOfOtherGroups(globalDir, new Set([pkg.hash]))
  const hashLink = getHashLink(globalDir, pkg.hash)
  const activatedBins = await activateGlobalInstall({
    installDir,
    hashLink,
    globalBinDir,
    pkgs,
    binsToSkip,
  })
  await cleanupReplacedGlobalInstalls({
    groups: [pkg],
    globalDir,
    globalBinDir,
    activeHash: pkg.hash,
    activatedBins,
    protectedBins,
  })
  await opts.updateResolutionPolicyManifest?.(resolutionPolicyViolations, globalDir)
}

/**
 * Installs `depSpecs` into `installDir`, which the caller has already created
 * under the global packages dir. The manifest and lockfile are written there;
 * with `lockfileOnly` nothing else is, so `node_modules` stays absent.
 */
async function installGroup (
  opts: GlobalUpdateOptions & { lockfileOnly?: boolean },
  installDir: string,
  depSpecs: string[]
): Promise<InstallGlobalPackagesResult> {
  const include = {
    dependencies: true,
    devDependencies: false,
    optionalDependencies: true,
  }
  return installGlobalPackages({
    ...opts,
    global: false,
    bin: path.join(installDir, 'node_modules/.bin'),
    dir: installDir,
    lockfileDir: installDir,
    rootProjectManifestDir: installDir,
    rootProjectManifest: undefined,
    saveProd: true,
    saveDev: false,
    saveOptional: false,
    savePeer: false,
    workspaceDir: undefined,
    sharedWorkspaceLockfile: false,
    lockfileOnly: opts.lockfileOnly ?? false,
    include,
    includeDirect: include,
    allowBuilds: opts.allowBuilds ?? {},
    omitSummaryLog: true,
  }, depSpecs)
}

/**
 * The selectors that reinstall a group. With `--latest` a plain version spec is
 * dropped so the newest release is picked; `pins` holds back the aliases that
 * would otherwise move backwards.
 */
function depSpecsForUpdate (
  dependencies: Record<string, string>,
  latest?: boolean,
  pins: ReadonlyMap<string, string> = new Map()
): string[] {
  return Object.entries(dependencies).map(([alias, spec]) => {
    const pin = pins.get(alias)
    if (pin != null) return `${alias}@${pin}`
    return latest && isPlainVersionSpec(spec) ? alias : `${alias}@${spec}`
  })
}

/**
 * The version to hold each dependency of `pkg` at, for the ones an update would
 * otherwise move backwards. `--latest` resolves the `latest` dist-tag, which
 * points at an older release than the one installed whenever that came from
 * another tag, or from a major that has not been promoted to `latest` yet.
 *
 * The versions are resolved into `installDir` without installing anything, so a
 * release that is about to be rejected never gets the chance to run its
 * lifecycle scripts. The install that follows reuses the lockfile written here
 * and only re-resolves what a pin changes.
 *
 * Only plain version dependencies are considered: every other spec form says
 * where the package comes from, so holding one at a bare version would resolve
 * a different package from the default registry.
 */
async function pinsForDowngrades (
  opts: GlobalUpdateOptions,
  installDir: string,
  pkg: GlobalPackageInfo
): Promise<Map<string, string>> {
  const pins = new Map<string, string>()
  // Only `--latest` can pick a version outside the recorded range, and only a
  // plain version spec is dropped for it. Everything else resolves within a
  // range the installed version already satisfies, so nothing below — not even
  // reading the group's installed versions — is worth doing.
  if (opts.latest !== true) return pins
  const versionsBefore = new Map(
    (await getGlobalPackageDetails(pkg))
      .filter(({ alias }) => isPlainVersionSpec(pkg.dependencies[alias] ?? ''))
      .map(({ alias, version }) => [alias, version])
  )
  // Nothing to compare a resolution against, so nothing to resolve.
  if (versionsBefore.size === 0) return pins

  const { resolvedVersions } = await installGroup(
    { ...opts, lockfileOnly: true },
    installDir,
    depSpecsForUpdate(pkg.dependencies, opts.latest)
  )
  for (const [alias, before] of versionsBefore) {
    const resolved = resolvedVersions[alias]
    if (semver.valid(before) == null || semver.valid(resolved) == null) continue
    if (semver.lt(resolved, before)) {
      pins.set(alias, before)
    }
  }
  return pins
}

// Only a plain version range may be dropped in favor of the bare alias.
// Every other spec form (`link:`, `file:`, a git or tarball URL, an `npm:`
// alias, a named registry) also says where the package comes from, so the
// alias alone would be resolved from the default registry: a different
// package gets installed, or the lookup 404s and aborts the groups that
// have not been updated yet.
function isPlainVersionSpec (spec: string): boolean {
  return !spec.includes(':')
}
