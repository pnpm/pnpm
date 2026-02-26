import fs from 'fs'
import path from 'path'
import util from 'util'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'
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

  // Parse aliases from params
  const aliases: string[] = []
  for (const param of params) {
    const parsed = parseWantedDependency(param)
    if (parsed.alias) {
      aliases.push(parsed.alias)
    } else {
      // For non-npm packages (tarballs, git repos etc.), use the raw param
      aliases.push(param)
    }
  }

  // Check if any of these aliases are already installed in a different group
  // and collect groups to remove
  await removeExistingGlobalInstalls(globalDir, globalBinDir, aliases)

  // Compute cache key from aliases + registries
  const cacheHash = createGlobalCacheKey({
    aliases,
    registries: opts.registries,
  })

  const hashDir = getHashDir(globalDir, cacheHash)
  fs.mkdirSync(hashDir, { recursive: true })
  const installDir = getPrepareDir(hashDir)
  fs.mkdirSync(installDir, { recursive: true })

  // Convert allowBuild array to allowBuilds Record (same conversion as add.handler)
  let allowBuilds = opts.allowBuilds ?? {}
  if (opts.allowBuild?.length) {
    allowBuilds = { ...allowBuilds }
    for (const pkg of opts.allowBuild) {
      allowBuilds[pkg] = true
    }
  }

  // Install packages into isolated directory (same pattern as dlx)
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
  const pkgJson = JSON.parse(fs.readFileSync(path.join(installDir, 'package.json'), 'utf-8'))
  const depNames = Object.keys(pkgJson.dependencies ?? {})
  const manifests = await Promise.all(
    depNames.map((depName) => readPackageJsonFromDir(path.join(installDir, 'node_modules', depName)))
  )
  return depNames.map((depName, i) => ({
    manifest: manifests[i] as DependencyManifest,
    location: path.join(installDir, 'node_modules', depName),
  }))
}
