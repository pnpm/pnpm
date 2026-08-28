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
import { installGlobalPackages, type ResolutionPolicyViolation } from './installGlobalPackages.js'
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
  const versionsBefore = new Map(
    (await getGlobalPackageDetails(pkg)).map(({ alias, version }) => [alias, version])
  )
  let install = await installGroup(opts, globalDir, depSpecsForUpdate(pkg.dependencies, opts.latest))
  // An update must never move a package backwards, and `--latest` can: the
  // `latest` dist-tag points at an older release than the one installed
  // whenever that came from another tag, or from a major that has not been
  // promoted to `latest` yet. Reinstall those held at the version that is
  // already there, so the rest of the group still gets its update.
  const pins = downgradedVersions(pkg.dependencies, versionsBefore, install.pkgs)
  if (pins.size > 0) {
    await fs.promises.rm(install.installDir, { recursive: true, force: true })
    install = await installGroup(opts, globalDir, depSpecsForUpdate(pkg.dependencies, opts.latest, pins))
  }
  const { installDir, pkgs, ignoredBuilds, resolutionPolicyViolations } = install

  await promptApproveGlobalBuilds({
    globalPkgDir: globalDir,
    installDir,
    ignoredBuilds,
    allowBuilds: opts.allowBuilds ?? {},
    inheritedOpts: opts,
  }, commands)

  // Check for bin name conflicts with other global packages
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

interface GroupInstall {
  installDir: string
  pkgs: InstalledGroupPackage[]
  ignoredBuilds: Awaited<ReturnType<typeof installGlobalPackages>>['ignoredBuilds']
  resolutionPolicyViolations: ResolutionPolicyViolation[]
}

/** Installs `depSpecs` into a fresh directory under the global packages dir. */
async function installGroup (
  opts: GlobalUpdateOptions,
  globalDir: string,
  depSpecs: string[]
): Promise<GroupInstall> {
  const installDir = createInstallDir(globalDir)
  const include = {
    dependencies: true,
    devDependencies: false,
    optionalDependencies: true,
  }
  const { ignoredBuilds, resolutionPolicyViolations } = await installGlobalPackages({
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
    lockfileOnly: false,
    include,
    includeDirect: include,
    allowBuilds: opts.allowBuilds ?? {},
    omitSummaryLog: true,
  }, depSpecs)
  return {
    installDir,
    pkgs: await readInstalledPackages(installDir),
    ignoredBuilds,
    resolutionPolicyViolations,
  }
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
 * The versions `installed` moved backwards from, by alias. Only plain version
 * dependencies are considered: every other spec form says where the package
 * comes from, so holding it at a bare version would resolve a different package
 * from the default registry.
 */
function downgradedVersions (
  dependencies: Record<string, string>,
  versionsBefore: ReadonlyMap<string, string>,
  installed: readonly InstalledGroupPackage[]
): Map<string, string> {
  const pins = new Map<string, string>()
  for (const { alias, manifest } of installed) {
    const before = versionsBefore.get(alias)
    const spec = dependencies[alias]
    if (before == null || spec == null || !isPlainVersionSpec(spec)) continue
    if (semver.valid(before) == null || semver.valid(manifest.version) == null) continue
    if (semver.lt(manifest.version, before)) {
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
