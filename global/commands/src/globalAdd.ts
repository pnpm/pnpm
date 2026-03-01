import fs from 'fs'
import path from 'path'
import {
  cleanOrphanedInstallDirs,
  createGlobalCacheKey,
  createInstallDir,
  findGlobalPackage,
  getHashLink,
  getInstalledBinNames,
} from '@pnpm/global.packages'
import { linkBinsOfPackages } from '@pnpm/link-bins'
import { removeBin } from '@pnpm/remove-bins'
import { readPackageJsonFromDirRawSync } from '@pnpm/read-package-json'
import isSubdir from 'is-subdir'
import symlinkDir from 'symlink-dir'
import { type CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { approveBuilds } from '@pnpm/exec.build-commands'
import { installGlobalPackages } from './installGlobalPackages.js'

type ApproveBuildsHandlerOpts = Parameters<typeof approveBuilds.handler>[0]
import { checkGlobalBinConflicts } from './checkGlobalBinConflicts.js'
import { readInstalledPackages } from './readInstalledPackages.js'

export type GlobalAddOptions = CreateStoreControllerOptions & {
  bin?: string
  globalPkgDir?: string
  registries: Record<string, string>
  allowBuild?: string[]
  allowBuilds?: Record<string, string | boolean>
  saveExact?: boolean
  savePrefix?: string
  supportedArchitectures?: { libc?: string[] }
  rootProjectManifest?: { pnpm?: { supportedArchitectures?: { libc?: string[] } } }
}

export async function handleGlobalAdd (
  opts: GlobalAddOptions,
  params: string[]
): Promise<void> {
  const globalDir = opts.globalPkgDir!
  const globalBinDir = opts.bin!
  cleanOrphanedInstallDirs(globalDir)

  // Install into a new directory first, then read the resolved aliases
  // from the resulting package.json. This is more reliable than parsing
  // aliases from CLI params (which may be tarballs, git URLs, etc.).
  const installDir = createInstallDir(globalDir)

  // Convert allowBuild array to allowBuilds Record (same conversion as add.handler)
  let allowBuilds = opts.allowBuilds ?? {}
  if (opts.allowBuild?.length) {
    allowBuilds = { ...allowBuilds }
    for (const pkg of opts.allowBuild) {
      allowBuilds[pkg] = true
    }
  }

  const include = {
    dependencies: true,
    devDependencies: false,
    optionalDependencies: true,
  }
  const fetchFullMetadata = (opts.supportedArchitectures?.libc ?? opts.rootProjectManifest?.pnpm?.supportedArchitectures?.libc) && true

  const makeInstallOpts = (dir: string, builds: Record<string, string | boolean>) => ({
    ...opts,
    global: false,
    bin: path.join(dir, 'node_modules/.bin'),
    dir,
    lockfileDir: dir,
    rootProjectManifestDir: dir,
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
    allowBuilds: builds,
  })

  const ignoredBuilds = await installGlobalPackages(makeInstallOpts(installDir, allowBuilds), params)

  // If any packages had their builds skipped, prompt the user to approve them
  // (reuses the same interactive flow as `pnpm approve-builds`)
  if (ignoredBuilds?.size && process.stdin.isTTY) {
    await approveBuilds.handler({
      ...opts,
      modulesDir: path.join(installDir, 'node_modules'),
      dir: installDir,
      lockfileDir: installDir,
      rootProjectManifest: undefined,
      rootProjectManifestDir: installDir,
      workspaceDir: opts.globalPkgDir!,
      global: false,
      pending: false,
      allowBuilds,
    } as ApproveBuildsHandlerOpts)
  }

  // Read resolved aliases from the installed package.json
  const pkgJson = readPackageJsonFromDirRawSync(installDir)
  const aliases = Object.keys(pkgJson.dependencies ?? {})

  // Check for bin name conflicts with other global packages
  // (must happen before removeExistingGlobalInstalls so we don't lose existing packages on failure)
  const pkgs = await readInstalledPackages(installDir)
  try {
    await checkGlobalBinConflicts({
      globalDir,
      globalBinDir,
      newPkgs: pkgs,
      shouldSkip: (pkg) => aliases.some((alias) => alias in pkg.dependencies),
    })
  } catch (err) {
    await fs.promises.rm(installDir, { recursive: true, force: true })
    throw err
  }

  // Remove any existing global installations of these aliases
  await removeExistingGlobalInstalls(globalDir, globalBinDir, aliases)

  // Compute cache key and create hash symlink pointing to install dir
  const cacheHash = createGlobalCacheKey({
    aliases,
    registries: opts.registries,
  })
  const hashLink = getHashLink(globalDir, cacheHash)
  await symlinkDir(installDir, hashLink, { overwrite: true })

  // Link bins from installed packages into global bin dir
  await linkBinsOfPackages(pkgs, globalBinDir)
}

async function removeExistingGlobalInstalls (
  globalDir: string,
  globalBinDir: string,
  aliases: string[]
): Promise<void> {
  // Collect unique groups to remove (dedup by hash)
  const groupsToRemove = new Map<string, ReturnType<typeof getInstalledBinNames>>()
  for (const alias of aliases) {
    const existing = findGlobalPackage(globalDir, alias)
    if (existing && !groupsToRemove.has(existing.hash)) {
      groupsToRemove.set(existing.hash, getInstalledBinNames(existing))
    }
  }

  // Remove all groups in parallel
  await Promise.all(
    [...groupsToRemove.entries()].map(async ([hash, binNamesPromise]) => {
      const binNames = await binNamesPromise
      await Promise.all(binNames.map((binName) => removeBin(path.join(globalBinDir, binName))))
      // Remove both the hash symlink and the install dir it points to
      const hashLink = getHashLink(globalDir, hash)
      let installDir: string | null = null
      try {
        installDir = fs.realpathSync(hashLink)
      } catch {}
      await fs.promises.rm(hashLink, { force: true })
      if (installDir && isSubdir(globalDir, installDir)) {
        await fs.promises.rm(installDir, { recursive: true, force: true })
      }
    })
  )
}
