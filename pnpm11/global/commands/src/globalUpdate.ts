import fs from 'node:fs'
import path from 'node:path'

import type { CommandHandlerMap } from '@pnpm/cli.command'
import { summaryLogger } from '@pnpm/core-loggers'
import {
  cleanOrphanedInstallDirs,
  createInstallDir,
  getHashLink,
  type GlobalPackageInfo,
  scanGlobalPackages,
} from '@pnpm/global.packages'
import type { CreateStoreControllerOptions } from '@pnpm/store.connection-manager'

import { getBinNamesOfOtherGroups } from './binOwnership.js'
import { checkGlobalBinConflicts } from './checkGlobalBinConflicts.js'
import { activateGlobalInstall, cleanupReplacedGlobalInstalls } from './globalActivation.js'
import { installGlobalPackages, type ResolutionPolicyViolation } from './installGlobalPackages.js'
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
  const allPackages = scanGlobalPackages(globalDir)

  if (allPackages.length === 0) {
    return 'No global packages found'
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

  // When --latest, just pass alias names to get the latest version.
  // Otherwise, pass alias@spec to update within the existing range.
  const depSpecs = Object.entries(pkg.dependencies).map(
    ([alias, spec]) => opts.latest && isPlainVersionSpec(spec) ? alias : `${alias}@${spec}`
  )

  const include = {
    dependencies: true,
    devDependencies: false,
    optionalDependencies: true,
  }
  const allowBuilds = opts.allowBuilds ?? {}

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
    allowBuilds,
    omitSummaryLog: true,
  }, depSpecs)

  await promptApproveGlobalBuilds({
    globalPkgDir: globalDir,
    installDir,
    ignoredBuilds,
    allowBuilds,
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

// Only a plain version range may be dropped in favor of the bare alias.
// Every other spec form (`link:`, `file:`, a git or tarball URL, an `npm:`
// alias, a named registry) also says where the package comes from, so the
// alias alone would be resolved from the default registry: a different
// package gets installed, or the lookup 404s and aborts the groups that
// have not been updated yet.
function isPlainVersionSpec (spec: string): boolean {
  return !spec.includes(':')
}
