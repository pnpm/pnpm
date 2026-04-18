import fs from 'node:fs'
import path from 'node:path'

import { linkBinsOfPackages } from '@pnpm/bins.linker'
import { removeBin } from '@pnpm/bins.remover'
import type { CommandHandlerMap } from '@pnpm/cli.command'
import {
  cleanOrphanedInstallDirs,
  createGlobalCacheKey,
  createInstallDir,
  findGlobalPackage,
  getHashLink,
  getInstalledBinNames,
} from '@pnpm/global.packages'
import { readPackageJsonFromDirRawSync } from '@pnpm/pkg-manifest.reader'
import type { CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import { isSubdir } from 'is-subdir'
import { symlinkDir } from 'symlink-dir'

import { checkGlobalBinConflicts } from './checkGlobalBinConflicts.js'
import { installGlobalPackages } from './installGlobalPackages.js'
import { promptApproveGlobalBuilds } from './promptApproveGlobalBuilds.js'
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
  rootProjectManifest?: unknown
}

export async function handleGlobalAdd (
  opts: GlobalAddOptions,
  params: string[],
  commands: CommandHandlerMap
): Promise<void> {
  // Resolve relative path selectors to absolute paths before the working
  // directory is changed to the global install dir, otherwise "." or
  // "./foo" would resolve against the temp install directory.
  params = params.map((param) => resolveLocalParam(param, opts.dir))
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
  const fetchFullMetadata = opts.supportedArchitectures?.libc != null && true

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

  await promptApproveGlobalBuilds({
    globalPkgDir: globalDir,
    installDir,
    ignoredBuilds,
    allowBuilds,
    inheritedOpts: opts,
  }, commands)

  // Read resolved aliases from the installed package.json
  const pkgJson = readPackageJsonFromDirRawSync(installDir)
  const aliases = Object.keys(pkgJson.dependencies ?? {})

  // Check for bin name conflicts with other global packages
  // (must happen before removeExistingGlobalInstalls so we don't lose existing packages on failure)
  const pkgs = await readInstalledPackages(installDir)
  let binsToSkip: Set<string>
  try {
    binsToSkip = await checkGlobalBinConflicts({
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
  await linkBinsOfPackages(pkgs, globalBinDir, { excludeBins: binsToSkip })
}

function resolveLocalParam (param: string, baseDir: string): string {
  for (const prefix of ['file:', 'link:']) {
    if (param.startsWith(prefix)) {
      const rest = param.slice(prefix.length)
      if (rest.startsWith('.')) {
        return prefix + path.resolve(baseDir, rest)
      }
      return param
    }
  }
  if (param.startsWith('.')) {
    return path.resolve(baseDir, param)
  }
  return param
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
