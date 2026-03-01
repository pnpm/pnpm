import fs from 'fs'
import path from 'path'
import {
  cleanOrphanedInstallDirs,
  createGlobalCacheKey,
  createInstallDir,
  findGlobalPackage,
  getHashLink,
  getInstalledBinNames,
} from '@pnpm/global-packages'
import { linkBinsOfPackages } from '@pnpm/link-bins'
import { readPackageJsonFromDir, readPackageJsonFromDirRawSync } from '@pnpm/read-package-json'
import { removeBin } from '@pnpm/remove-bins'
import { type DependencyManifest } from '@pnpm/types'
import isSubdir from 'is-subdir'
import symlinkDir from 'symlink-dir'
import { type AddCommandOptions } from './add.js'
import { checkGlobalBinConflicts } from './checkGlobalBinConflicts.js'
import { installDeps } from './installDeps.js'
import { getFetchFullMetadata } from './getFetchFullMetadata.js'

export async function handleGlobalAdd (
  opts: AddCommandOptions,
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
  await installDeps({
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
    fetchFullMetadata: getFetchFullMetadata(opts),
    include,
    includeDirect: include,
    allowBuilds,
  }, params)

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

async function readInstalledPackages (installDir: string): Promise<Array<{ manifest: DependencyManifest, location: string }>> {
  const pkgJson = readPackageJsonFromDirRawSync(installDir)
  const depNames = Object.keys(pkgJson.dependencies ?? {})
  const manifests = await Promise.all(
    depNames.map((depName) => readPackageJsonFromDir(path.join(installDir, 'node_modules', depName)))
  )
  return depNames.map((depName, i) => ({
    manifest: manifests[i] as DependencyManifest,
    location: path.join(installDir, 'node_modules', depName),
  }))
}
