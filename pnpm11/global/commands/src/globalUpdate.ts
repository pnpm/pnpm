import fs from 'node:fs'
import path from 'node:path'
import { isDeepStrictEqual } from 'node:util'

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
import { readModulesManifest } from '@pnpm/installing.modules-yaml'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { globalWarn, logger } from '@pnpm/logger'
import { localFilePath } from '@pnpm/resolving.local-resolver'
import type { CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import type { ProjectManifest } from '@pnpm/types'
import semver from 'semver'

import { getGlobalBinOwnership } from './binOwnership.js'
import { checkGlobalBinConflicts } from './checkGlobalBinConflicts.js'
import { cleanupFailedGlobalInstall } from './cleanupFailedGlobalInstall.js'
import { activateGlobalInstall, cleanupReplacedGlobalInstalls, getActualBinNames } from './globalActivation.js'
import {
  installGlobalPackages,
  type InstallGlobalPackagesResult,
  type ResolutionPolicyViolation,
} from './installGlobalPackages.js'
import { hasPnpmCliDependency } from './pnpmCliPackages.js'
import { promptApproveGlobalBuilds } from './promptApproveGlobalBuilds.js'
import { type InstalledGroupPackage, readInstalledPackages } from './readInstalledPackages.js'

export type GlobalUpdateOptions = CreateStoreControllerOptions & {
  bin?: string
  globalPkgDir?: string
  latest?: boolean
  allowBuilds?: Record<string, string | boolean>
  saveExact?: boolean
  savePrefix?: string
  rootProjectManifest?: ProjectManifest
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
  cleanOrphanedInstallDirs(globalDir)
  const packagesToUpdate = selectPackagesToUpdate(opts, params)
  if (typeof packagesToUpdate === 'string') return packagesToUpdate

  // Update each package group sequentially to avoid overwhelming the system

  const { checked, changed } = await updateGlobalPackageGroups(
    { opts, globalDir, globalBinDir: opts.bin!, commands },
    packagesToUpdate
  )
  if (checked && !changed) {
    logger.info({ message: 'Already up to date', prefix: opts.dir })
  }
  summaryLogger.debug({ prefix: globalDir })
  return undefined
}

/**
 * The groups `update -g` should update, or the message to print when there
 * is nothing to update.
 */
function selectPackagesToUpdate (opts: GlobalUpdateOptions, params: string[]): GlobalPackageInfo[] | string {
  const scannedPackages = scanGlobalPackages(opts.globalPkgDir!)

  if (scannedPackages.length === 0) {
    return 'No global packages found'
  }
  const allPackages = scannedPackages.filter((pkg) => !hasPnpmCliDependency(pkg))
  if (allPackages.length === 0) {
    return 'No global packages to update. Run "pnpm self-update" to update pnpm itself.'
  }

  let packagesToUpdate = allPackages
  if (params.length > 0) {
    packagesToUpdate = allPackages.filter((pkg) =>
      params.some((param) => Object.hasOwn(pkg.dependencies, param))
    )
    if (packagesToUpdate.length === 0) {
      return 'No matching global packages found'
    }
  }
  const selectedPackageHashes = opts.selectedPackageHashes
  if (selectedPackageHashes) {
    packagesToUpdate = packagesToUpdate.filter(({ hash }) => selectedPackageHashes.has(hash))
  }
  return packagesToUpdate
}

interface GlobalUpdateContext {
  opts: GlobalUpdateOptions
  globalDir: string
  globalBinDir: string
  commands: CommandHandlerMap
}

async function updateGlobalPackageGroups (
  ctx: GlobalUpdateContext,
  packagesToUpdate: GlobalPackageInfo[]
): Promise<{ checked: boolean, changed: boolean }> {
  let checked = false
  let changed = false
  for (const pkg of packagesToUpdate) {
    const missingSourceWarning = missingFileSourceWarning(pkg)
    if (missingSourceWarning != null) {
      globalWarn(missingSourceWarning)
      continue
    }
    checked = true
    changed = await updateGlobalPackageGroup(ctx, pkg) || changed // eslint-disable-line no-await-in-loop -- groups share the global bin directory, so they are updated one at a time
  }
  return { checked, changed }
}

/**
 * The warning `update -g` prints for a group it skips because the `file:`
 * source one of its dependencies was installed from is gone. Reinstalling the
 * group would fail with `ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND`, and the group
 * still works from what it installed, so the other groups are updated instead.
 */
function missingFileSourceWarning (pkg: GlobalPackageInfo): string | undefined {
  for (const [alias, spec] of Object.entries(pkg.dependencies)) {
    const source = localFilePath(spec, pkg.installDir)
    if (source != null && fs.statSync(source, { throwIfNoEntry: false }) == null) {
      return `Skipped updating ${Object.keys(pkg.dependencies).join(', ')} because "${source}" no longer exists. ` +
        `Reinstall ${alias} from an existing location, or remove it with "pnpm remove -g ${alias}".`
    }
  }
  return undefined
}

async function updateGlobalPackageGroup (
  ctx: GlobalUpdateContext,
  pkg: GlobalPackageInfo
): Promise<boolean> {
  const { opts, globalDir, commands } = ctx
  const installDir = createInstallDir(globalDir)
  const groupOpts = withSharedApprovals(opts)
  const { depSpecs, comparison } = await resolveGroupUpdate(groupOpts, installDir, pkg)

  // Equal lockfiles mean no new packages, not that the tree they describe is
  // still on disk. The modules manifest is what an install leaves behind.
  const activeModules = await readModulesManifest(path.join(pkg.installDir, 'node_modules'))
  if (activeModules != null && await lockfilesAreEqual(pkg.installDir, installDir)) {
    await fs.promises.rm(installDir, { recursive: true, force: true })
    await promptApproveGlobalBuilds({
      globalPkgDir: globalDir,
      installDir: pkg.installDir,
      ignoredBuilds: activeModules.ignoredBuilds,
      allowBuilds: opts.allowBuilds ?? {},
      inheritedOpts: opts,
    }, commands)
    await opts.updateResolutionPolicyManifest?.(comparison.resolutionPolicyViolations, globalDir)
    return false
  }

  const { ignoredBuilds, resolutionPolicyViolations } = await installGroup(groupOpts, installDir, depSpecs)

  await promptApproveGlobalBuilds({
    globalPkgDir: globalDir,
    installDir,
    ignoredBuilds,
    allowBuilds: opts.allowBuilds ?? {},
    inheritedOpts: opts,
  }, commands)

  await activateUpdatedGroup(ctx, { pkg, installDir })
  await opts.updateResolutionPolicyManifest?.(resolutionPolicyViolations, globalDir)
  return true
}

/**
 * The selectors to reinstall `pkg` with, and a lockfile-only resolution of
 * them in `installDir` to compare against the active install.
 */
async function resolveGroupUpdate (
  groupOpts: GlobalUpdateOptions,
  installDir: string,
  pkg: GlobalPackageInfo
): Promise<{ depSpecs: string[], comparison: InstallGlobalPackagesResult }> {
  const downgradeCheck = await pinsForDowngrades(groupOpts, installDir, pkg)
  const depSpecs = depSpecsForUpdate(pkg.dependencies, groupOpts.latest, downgradeCheck.pins)
  const comparison = downgradeCheck.candidate != null && downgradeCheck.pins.size === 0
    ? downgradeCheck.candidate
    : await installGroup(
      { ...groupOpts, lockfileOnly: true, groupDependencies: pkg.dependencies },
      installDir,
      depSpecs
    )
  return { depSpecs, comparison }
}

async function activateUpdatedGroup (
  ctx: GlobalUpdateContext,
  group: { pkg: GlobalPackageInfo, installDir: string }
): Promise<void> {
  const { globalDir, globalBinDir } = ctx
  const { pkg, installDir } = group
  const pkgs = await readInstalledPackages(installDir)
  const { binsToSkip, retainedBinNames, ownership } = await planUpdatedGroupBins(ctx, { pkg, installDir, pkgs })
  const hashLink = getHashLink(globalDir, pkg.hash)
  const activatedBins = await activateGlobalInstall({
    installDir,
    hashLink,
    globalBinDir,
    pkgs,
    binsToSkip,
    requiredBinNames: retainedBinNames,
  })
  await cleanupReplacedGlobalInstalls({
    groups: ownership.groups,
    globalDir,
    globalBinDir,
    activeHash: pkg.hash,
    activatedBins,
    protectedBins: ownership.protectedBins,
  })
}

interface UpdatedGroupBins {
  binsToSkip: Set<string>
  retainedBinNames: Set<string>
  ownership: Awaited<ReturnType<typeof getGlobalBinOwnership>>
}

async function planUpdatedGroupBins (
  ctx: GlobalUpdateContext,
  group: { pkg: GlobalPackageInfo, installDir: string, pkgs: InstalledGroupPackage[] }
): Promise<UpdatedGroupBins> {
  const { pkg, installDir, pkgs } = group
  try {
    const binsToSkip = await checkGlobalBinConflicts({
      globalDir: ctx.globalDir,
      globalBinDir: ctx.globalBinDir,
      newPkgs: pkgs,
      shouldSkip: (existingPkg) => existingPkg.hash === pkg.hash,
    })
    const retainedBinNames = await getActualBinNames({ pkgs, binsToSkip })
    const ownership = await getGlobalBinOwnership(
      ctx.globalDir,
      [pkg],
      retainedBinNames
    )
    return { binsToSkip, retainedBinNames, ownership }
  } catch (err) {
    return cleanupFailedGlobalInstall(installDir, err)
  }
}

function withSharedApprovals (opts: GlobalUpdateOptions): GlobalUpdateOptions {
  const handleResolutionPolicyViolations = opts.handleResolutionPolicyViolations
  if (handleResolutionPolicyViolations == null) return opts
  const approved = new Set<string>()
  return {
    ...opts,
    handleResolutionPolicyViolations: async (violations: readonly ResolutionPolicyViolation[]): Promise<void> => {
      const pending = violations.filter(({ name, version }) => !approved.has(`${name}@${version}`))
      if (pending.length === 0) return
      await handleResolutionPolicyViolations(pending)
      for (const { name, version } of pending) {
        approved.add(`${name}@${version}`)
      }
    },
  }
}

type InstallGroupOptions = GlobalUpdateOptions & {
  lockfileOnly?: boolean
  groupDependencies?: Record<string, string>
}

/**
 * Installs `depSpecs` into `installDir`, which the caller has already created
 * under the global packages dir. The manifest and lockfile are written there;
 * with `lockfileOnly` nothing else is, so `node_modules` stays absent.
 *
 * `groupDependencies` is the manifest the install starts from. The first call
 * into a fresh `installDir` passes the group's recorded dependencies, without
 * which the lockfile would carry no specifiers to compare against the group's
 * own. Omitting it starts from the manifest written by an earlier call.
 */
async function installGroup (
  opts: InstallGroupOptions,
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
    rootProjectManifest: opts.groupDependencies == null ? undefined : { dependencies: opts.groupDependencies },
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
): Promise<{ candidate?: InstallGlobalPackagesResult, pins: Map<string, string> }> {
  const pins = new Map<string, string>()
  // Only `--latest` can pick a version outside the recorded range, and only a
  // plain version spec is dropped for it. Everything else resolves within a
  // range the installed version already satisfies, so nothing below — not even
  // reading the group's installed versions — is worth doing.
  if (opts.latest !== true) return { pins }
  const versionsBefore = new Map(
    (await getGlobalPackageDetails(pkg))
      .filter(({ alias }) => isPlainVersionSpec(pkg.dependencies[alias] ?? ''))
      .map(({ alias, version }) => [alias, version])
  )
  // Nothing to compare a resolution against, so nothing to resolve.
  if (versionsBefore.size === 0) return { pins }

  const candidate = await installGroup(
    { ...opts, lockfileOnly: true, groupDependencies: pkg.dependencies },
    installDir,
    depSpecsForUpdate(pkg.dependencies, opts.latest)
  )
  const { resolvedVersions } = candidate
  for (const [alias, before] of versionsBefore) {
    const resolved = resolvedVersions[alias]
    if (semver.valid(before) == null || semver.valid(resolved) == null) continue
    if (semver.lt(resolved, before)) {
      pins.set(alias, before)
    }
  }
  return { candidate, pins }
}

async function lockfilesAreEqual (activeDir: string, candidateDir: string): Promise<boolean> {
  try {
    const [active, candidate] = await Promise.all([
      readWantedLockfile(activeDir, { ignoreIncompatible: false }),
      readWantedLockfile(candidateDir, { ignoreIncompatible: false }),
    ])
    return active != null && candidate != null && isDeepStrictEqual(active, candidate)
  } catch {
    return false
  }
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
