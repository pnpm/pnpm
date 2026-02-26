import fs from 'fs'
import path from 'path'
import util from 'util'
import {
  getGlobalDir,
  getHashDir,
  getPrepareDir,
  scanGlobalPackages,
  type GlobalPackageInfo,
} from '@pnpm/global-packages'
import { linkBinsOfPackages } from '@pnpm/link-bins'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { type DependencyManifest } from '@pnpm/types'
import symlinkDir from 'symlink-dir'
import { type UpdateCommandOptions } from './update/index.js'
import { installDeps } from './installDeps.js'
import { getFetchFullMetadata } from './getFetchFullMetadata.js'

export async function handleGlobalUpdate (
  opts: UpdateCommandOptions,
  params: string[]
): Promise<string | undefined> {
  const pnpmHomeDir = opts.pnpmHomeDir
  if (!pnpmHomeDir) {
    throw new Error('pnpmHomeDir is required for global updates')
  }
  const globalDir = getGlobalDir(pnpmHomeDir)
  const globalBinDir = opts.bin!
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
    await updateGlobalPackageGroup(opts, globalDir, globalBinDir, pkg) // eslint-disable-line no-await-in-loop
  }
  return undefined
}

async function updateGlobalPackageGroup (
  opts: UpdateCommandOptions,
  globalDir: string,
  globalBinDir: string,
  pkg: GlobalPackageInfo
): Promise<void> {
  const hashDir = getHashDir(globalDir, pkg.hash)
  const installDir = getPrepareDir(hashDir)
  fs.mkdirSync(installDir, { recursive: true })

  // Re-install with latest versions
  const depSpecs = Object.entries(pkg.dependencies).map(
    ([alias, spec]) => `${alias}@${spec}`
  )

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
    allowNew: true,
    update: true,
    updateToLatest: opts.latest,
  }, depSpecs)

  // Swap pkg symlink to new install
  const pkgLink = path.join(hashDir, 'pkg')
  try {
    await symlinkDir(installDir, pkgLink, { overwrite: true })
  } catch (error) {
    if (!util.types.isNativeError(error) || !('code' in error) || (error.code !== 'EBUSY' && error.code !== 'EEXIST')) {
      throw error
    }
  }

  // Re-link bins
  const pkgs = await readInstalledPackages(installDir)
  await linkBinsOfPackages(pkgs, globalBinDir)
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
