import fs from 'node:fs'
import path from 'node:path'

import { linkBinsOfPackages } from '@pnpm/bins.linker'
import { removeBin } from '@pnpm/bins.remover'
import type { CommandHandlerMap } from '@pnpm/cli.command'
import {
  cleanOrphanedInstallDirs,
  createInstallDir,
  getHashLink,
  getInstalledBinNames,
  type GlobalPackageInfo,
  scanGlobalPackages,
} from '@pnpm/global.packages'
import type { CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import { isSubdir } from 'is-subdir'
import { symlinkDir } from 'symlink-dir'

import { checkGlobalBinConflicts } from './checkGlobalBinConflicts.js'
import { installGlobalPackages } from './installGlobalPackages.js'
import { promptApproveGlobalBuilds } from './promptApproveGlobalBuilds.js'
import { readInstalledPackages } from './readInstalledPackages.js'

export type GlobalUpdateOptions = CreateStoreControllerOptions & {
  bin?: string
  globalPkgDir?: string
  latest?: boolean
  allowBuilds?: Record<string, string | boolean>
  saveExact?: boolean
  savePrefix?: string
  supportedArchitectures?: { libc?: string[] }
  rootProjectManifest?: unknown
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
      params.some((p) => p in pkg.dependencies)
    )
    if (packagesToUpdate.length === 0) {
      return 'No matching global packages found'
    }
  } else {
    packagesToUpdate = allPackages
  }

  // Update each package group sequentially to avoid overwhelming the system

  for (const pkg of packagesToUpdate) {
    await updateGlobalPackageGroup(opts, globalDir, globalBinDir, pkg, commands) // eslint-disable-line no-await-in-loop
  }
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
    ([alias, spec]) => opts.latest ? alias : `${alias}@${spec}`
  )

  const include = {
    dependencies: true,
    devDependencies: false,
    optionalDependencies: true,
  }
  const fetchFullMetadata = opts.supportedArchitectures?.libc != null && true
  const allowBuilds = opts.allowBuilds ?? {}

  const ignoredBuilds = await installGlobalPackages({
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
    fetchFullMetadata,
    include,
    includeDirect: include,
    allowBuilds,
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

  // Remove stale bins from old installation before swapping
  const oldBinNames = await getInstalledBinNames(pkg)
  await Promise.all(oldBinNames.map((binName) => removeBin(path.join(globalBinDir, binName))))

  // Swap hash symlink to new install dir, then clean up old one
  const hashLink = getHashLink(globalDir, pkg.hash)
  const oldInstallDir = pkg.installDir
  await symlinkDir(installDir, hashLink, { overwrite: true })
  if (isSubdir(globalDir, oldInstallDir)) {
    await fs.promises.rm(oldInstallDir, { recursive: true, force: true })
  }

  // Link bins from new installation
  await linkBinsOfPackages(pkgs, globalBinDir, { excludeBins: binsToSkip })
}
