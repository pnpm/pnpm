import fs from 'fs'
import path from 'path'
import util from 'util'
import {
  createGlobalCacheKey,
  findGlobalPackage,
  getGlobalDir,
  getHashDir,
  getInstalledBinNames,
  getPrepareDir,
} from '@pnpm/global-packages'
import { linkBinsOfPackages } from '@pnpm/link-bins'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { removeBin } from '@pnpm/remove-bins'
import { type DependencyManifest } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'
import symlinkDir from 'symlink-dir'
import { type AddCommandOptions } from './add.js'
import { installDeps } from './installDeps.js'
import { getFetchFullMetadata } from './getFetchFullMetadata.js'

export async function handleGlobalAdd (
  opts: AddCommandOptions,
  params: string[]
): Promise<void> {
  const pnpmHomeDir = opts.pnpmHomeDir
  if (!pnpmHomeDir) {
    throw new Error('pnpmHomeDir is required for global installations')
  }
  const globalDir = getGlobalDir(pnpmHomeDir)
  const globalBinDir = opts.bin!

  // Install into a temporary directory first, then read the resolved aliases
  // from the resulting package.json. This is more reliable than parsing
  // aliases from CLI params (which may be tarballs, git URLs, etc.).
  const tmpDir = path.join(globalDir, `.tmp-${process.pid.toString(16)}-${Date.now().toString(16)}`)
  fs.mkdirSync(tmpDir, { recursive: true })

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
    bin: path.join(tmpDir, 'node_modules/.bin'),
    dir: tmpDir,
    lockfileDir: tmpDir,
    rootProjectManifestDir: tmpDir,
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
  const pkgJson = loadJsonFileSync<{ dependencies?: Record<string, string> }>(path.join(tmpDir, 'package.json'))
  const aliases = Object.keys(pkgJson.dependencies ?? {})

  // Remove any existing global installations of these aliases
  await removeExistingGlobalInstalls(globalDir, globalBinDir, aliases)

  // Compute cache key from resolved aliases + registries
  const cacheHash = createGlobalCacheKey({
    aliases,
    registries: opts.registries,
  })

  // Move the temp install into the proper hash directory
  const hashDir = getHashDir(globalDir, cacheHash)
  fs.mkdirSync(hashDir, { recursive: true })
  const installDir = getPrepareDir(hashDir)
  fs.renameSync(tmpDir, installDir)

  // Create/update pkg symlink
  const pkgLink = path.join(hashDir, 'pkg')
  try {
    await symlinkDir(installDir, pkgLink, { overwrite: true })
  } catch (error) {
    if (!util.types.isNativeError(error) || !('code' in error) || (error.code !== 'EBUSY' && error.code !== 'EEXIST')) {
      throw error
    }
  }

  // Link bins from installed packages into global bin dir
  const pkgs = await readInstalledPackages(installDir)
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
      await fs.promises.rm(getHashDir(globalDir, hash), { recursive: true, force: true })
    })
  )
}

async function readInstalledPackages (installDir: string): Promise<Array<{ manifest: DependencyManifest, location: string }>> {
  const pkgJson = loadJsonFileSync<{ dependencies?: Record<string, string> }>(path.join(installDir, 'package.json'))
  const depNames = Object.keys(pkgJson.dependencies ?? {})
  const manifests = await Promise.all(
    depNames.map((depName) => readPackageJsonFromDir(path.join(installDir, 'node_modules', depName)))
  )
  return depNames.map((depName, i) => ({
    manifest: manifests[i] as DependencyManifest,
    location: path.join(installDir, 'node_modules', depName),
  }))
}
