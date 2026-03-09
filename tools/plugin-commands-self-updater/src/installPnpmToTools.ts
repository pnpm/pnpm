import fs from 'fs'
import path from 'path'
import { getCurrentPackageName } from '@pnpm/cli-meta'
import { calcLeafGlobalVirtualStorePath } from '@pnpm/calc-dep-state'
import { readConfigLockfile } from '@pnpm/config.deps-installer'
import type { ConfigLockfile } from '@pnpm/config.deps-installer'
import { PnpmError } from '@pnpm/error'
import { linkBins } from '@pnpm/link-bins'
import { globalWarn } from '@pnpm/logger'
import type { StoreController } from '@pnpm/package-store'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import getNpmTarballUrl from 'get-npm-tarball-url'
import type { SelfUpdateCommandOptions } from './selfUpdate.js'

export interface InstallPnpmToToolsResult {
  binDir: string
  baseDir: string
  alreadyExisted: boolean
}

export interface InstallPnpmToToolsOptions extends SelfUpdateCommandOptions {
  storeController: StoreController
  storeDir: string
}

export async function installPnpmToTools (pnpmVersion: string, opts: InstallPnpmToToolsOptions): Promise<InstallPnpmToToolsResult> {
  const currentPkgName = getCurrentPackageName()

  // Check if already installed in global virtual store
  const existing = findPnpmInGlobalStore(opts.storeDir, currentPkgName, pnpmVersion)
  if (existing) {
    return { ...existing, alreadyExisted: true }
  }

  // Read config lockfile to get integrity
  const configLockfile = await readConfigLockfile(opts.rootProjectManifestDir)
  const pkgKey = `${currentPkgName}@${pnpmVersion}`
  const pkgInfo = configLockfile?.packages[pkgKey]

  if (!pkgInfo?.resolution || !('integrity' in pkgInfo.resolution) || !pkgInfo.resolution.integrity) {
    throw new PnpmError(
      'MISSING_PM_INTEGRITY',
      `Cannot find integrity for ${pkgKey} in pnpm-config-lock.yaml`
    )
  }

  const resolution = pkgInfo.resolution as { integrity: string, tarball?: string }
  const globalVirtualStoreDir = path.join(opts.storeDir, 'links')
  const fullPkgId = `${currentPkgName}@${pnpmVersion}:${resolution.integrity}`
  const relPath = calcLeafGlobalVirtualStorePath(fullPkgId, currentPkgName, pnpmVersion)
  const baseDir = path.join(globalVirtualStoreDir, relPath)
  const pkgDir = path.join(baseDir, 'node_modules', currentPkgName)
  const binDir = path.join(baseDir, 'bin')

  const registry = pickRegistryForPackage(opts.registries, currentPkgName)

  // Fetch and import the main package
  if (!fs.existsSync(path.join(pkgDir, 'package.json'))) {
    await fetchAndImportPackage(opts.storeController, opts.rootProjectManifestDir, pkgDir, {
      id: pkgKey,
      resolution: {
        integrity: resolution.integrity,
        tarball: resolution.tarball ?? getNpmTarballUrl(currentPkgName, pnpmVersion, { registry }),
      },
    })
  }

  // For @pnpm/exe, also install the platform binary
  if (currentPkgName === '@pnpm/exe' && configLockfile) {
    await installExePlatformBinary(pnpmVersion, configLockfile, opts, globalVirtualStoreDir, pkgDir)
  }

  // Create bin scripts
  await linkBins(path.join(baseDir, 'node_modules'), binDir, { warn: globalWarn })

  return {
    alreadyExisted: false,
    baseDir,
    binDir,
  }
}

/**
 * Finds an already-installed pnpm version in the global virtual store.
 * Scans the version directory for any hash subdirectory that has a bin/ dir.
 */
export function findPnpmInGlobalStore (
  storeDir: string,
  pkgName: string,
  version: string
): { baseDir: string, binDir: string } | null {
  const globalVirtualStoreDir = path.join(storeDir, 'links')
  const prefix = pkgName.startsWith('@') ? '' : '@/'
  const versionDir = path.join(globalVirtualStoreDir, `${prefix}${pkgName}`, version)

  let hashDirs: string[]
  try {
    hashDirs = fs.readdirSync(versionDir)
  } catch {
    return null
  }
  for (const hashDir of hashDirs) {
    const baseDir = path.join(versionDir, hashDir)
    const binDir = path.join(baseDir, 'bin')
    if (fs.existsSync(binDir)) {
      return { baseDir, binDir }
    }
  }
  return null
}

async function fetchAndImportPackage (
  store: StoreController,
  lockfileDir: string,
  targetDir: string,
  pkg: { id: string, resolution: { integrity: string, tarball: string } }
): Promise<void> {
  const { fetching } = await store.fetchPackage({
    force: true,
    lockfileDir,
    pkg: {
      id: pkg.id,
      resolution: pkg.resolution,
    },
  })
  const { files: filesResponse } = await fetching()
  await store.importPackage(targetDir, {
    force: true,
    requiresBuild: false,
    filesResponse,
  })
}

async function installExePlatformBinary (
  pnpmVersion: string,
  configLockfile: ConfigLockfile,
  opts: InstallPnpmToToolsOptions,
  globalVirtualStoreDir: string,
  exePkgDir: string
): Promise<void> {
  const platform = process.platform === 'win32'
    ? 'win'
    : process.platform === 'darwin'
      ? 'macos'
      : process.platform
  const arch = platform === 'win' && process.arch === 'ia32' ? 'x86' : process.arch
  const platformPkgName = `@pnpm/${platform}-${arch}`
  const platformPkgKey = `${platformPkgName}@${pnpmVersion}`
  const platformPkgInfo = configLockfile.packages[platformPkgKey]

  if (!platformPkgInfo?.resolution || !('integrity' in platformPkgInfo.resolution) || !platformPkgInfo.resolution.integrity) return

  const platformResolution = platformPkgInfo.resolution as { integrity: string, tarball?: string }
  const fullPkgId = `${platformPkgName}@${pnpmVersion}:${platformResolution.integrity}`
  const relPath = calcLeafGlobalVirtualStorePath(fullPkgId, platformPkgName, pnpmVersion)
  const platformPkgDir = path.join(globalVirtualStoreDir, relPath, 'node_modules', platformPkgName)

  if (!fs.existsSync(path.join(platformPkgDir, 'package.json'))) {
    const registry = pickRegistryForPackage(opts.registries, platformPkgName)
    await fetchAndImportPackage(opts.storeController, opts.rootProjectManifestDir, platformPkgDir, {
      id: platformPkgKey,
      resolution: {
        integrity: platformResolution.integrity,
        tarball: platformResolution.tarball ?? getNpmTarballUrl(platformPkgName, pnpmVersion, { registry }),
      },
    })
  }

  const executable = platform === 'win' ? 'pnpm.exe' : 'pnpm'
  const src = path.join(platformPkgDir, executable)
  if (!fs.existsSync(src)) return

  const dest = path.join(exePkgDir, executable)
  try {
    fs.unlinkSync(dest)
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }
  }
  fs.linkSync(src, dest)
  fs.chmodSync(dest, 0o755)

  if (platform === 'win') {
    const exePkgJsonPath = path.join(exePkgDir, 'package.json')
    const exePkg = JSON.parse(fs.readFileSync(exePkgJsonPath, 'utf8'))
    fs.writeFileSync(path.join(exePkgDir, 'pnpm'), 'This file intentionally left blank')
    exePkg.bin.pnpm = 'pnpm.exe'
    fs.writeFileSync(exePkgJsonPath, JSON.stringify(exePkg, null, 2))
  }
}
