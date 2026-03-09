import fs from 'fs'
import path from 'path'
import { getCurrentPackageName, packageManager } from '@pnpm/cli-meta'
import type { ConfigLockfile } from '@pnpm/config.deps-installer'
import {
  createGlobalCacheKey,
  createInstallDir,
  findGlobalPackage,
  getHashLink,
} from '@pnpm/global.packages'
import { headlessInstall } from '@pnpm/headless'
import { linkBins } from '@pnpm/link-bins'
import type { PackageSnapshot } from '@pnpm/lockfile.types'
import { globalWarn } from '@pnpm/logger'
import type { StoreController } from '@pnpm/package-store'
import type { DepPath, ProjectId, ProjectRootDir } from '@pnpm/types'
import symlinkDir from 'symlink-dir'
import type { SelfUpdateCommandOptions } from './selfUpdate.js'

export interface InstallPnpmToToolsResult {
  binDir: string
  baseDir: string
  alreadyExisted: boolean
}

export interface InstallPnpmToToolsOptions extends SelfUpdateCommandOptions {
  configLockfile: ConfigLockfile
  storeController: StoreController
  storeDir: string
}

export async function installPnpmToTools (pnpmVersion: string, opts: InstallPnpmToToolsOptions): Promise<InstallPnpmToToolsResult> {
  const currentPkgName = getCurrentPackageName()
  const globalDir = opts.globalPkgDir!

  // Check if already installed globally
  const existing = findGlobalPackage(globalDir, currentPkgName)
  if (existing) {
    const pkgJsonPath = path.join(existing.installDir, 'node_modules', currentPkgName, 'package.json')
    try {
      const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
      if (pkgJson.version === pnpmVersion) {
        const binDir = path.join(existing.installDir, 'bin')
        return { alreadyExisted: true, baseDir: existing.installDir, binDir }
      }
    } catch {}
  }

  const installDir = createInstallDir(globalDir)
  fs.writeFileSync(path.join(installDir, 'package.json'), JSON.stringify({
    dependencies: {
      [currentPkgName]: pnpmVersion,
    },
  }))

  try {
    const wantedLockfile = buildLockfileFromConfigLockfile(opts.configLockfile, currentPkgName, pnpmVersion)
    const binDir = path.join(installDir, 'bin')
    await headlessInstall({
      wantedLockfile,
      lockfileDir: installDir,
      storeController: opts.storeController,
      storeDir: opts.storeDir,
      registries: opts.registries,
      nodeLinker: 'hoisted',
      enableGlobalVirtualStore: true,
      ignoreScripts: true,
      ignoreDepScripts: true,
      force: false,
      engineStrict: false,
      currentEngine: {
        pnpmVersion: packageManager.version,
      },
      include: {
        dependencies: true,
        devDependencies: false,
        optionalDependencies: true,
      },
      selectedProjectDirs: [installDir],
      allProjects: {
        [installDir]: {
          binsDir: binDir,
          buildIndex: 0,
          manifest: {
            dependencies: {
              [currentPkgName]: pnpmVersion,
            },
          },
          modulesDir: path.join(installDir, 'node_modules'),
          id: '.' as ProjectId,
          rootDir: installDir as ProjectRootDir,
        },
      },
      hoistedDependencies: {},
      globalVirtualStoreDir: path.join(opts.storeDir, 'links'),
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      sideEffectsCacheRead: false,
      sideEffectsCacheWrite: false,
      rawConfig: {},
      unsafePerm: false,
      userAgent: '',
      packageManager: {
        name: packageManager.name,
        version: packageManager.version,
      },
      pruneStore: false,
      pendingBuilds: [],
      skipped: new Set(),
    })
    if (currentPkgName === '@pnpm/exe') {
      linkExePlatformBinary(installDir)
    }

    // Create bin scripts
    await linkBins(path.join(installDir, 'node_modules'), binDir, { warn: globalWarn })

    // Create hash symlink for the global packages system
    const cacheHash = createGlobalCacheKey({
      aliases: [currentPkgName],
      registries: opts.registries,
    })
    const hashLink = getHashLink(globalDir, cacheHash)
    await symlinkDir(installDir, hashLink, { overwrite: true })

    return {
      alreadyExisted: false,
      baseDir: installDir,
      binDir,
    }
  } catch (err: unknown) {
    try {
      fs.rmSync(installDir, { recursive: true, force: true })
    } catch {} // eslint-disable-line:no-empty
    throw err
  }
}

function buildLockfileFromConfigLockfile (
  configLockfile: ConfigLockfile,
  pkgName: string,
  version: string
) {
  const dependencies: Record<string, string> = {}
  dependencies[pkgName] = version

  // Merge packages and snapshots into PackageSnapshots (the in-memory format)
  const packages: Record<string, PackageSnapshot> = {}
  for (const [depPath, snapshot] of Object.entries(configLockfile.snapshots)) {
    packages[depPath as DepPath] = {
      ...snapshot,
      ...configLockfile.packages[depPath],
    }
  }

  return {
    lockfileVersion: configLockfile.lockfileVersion,
    importers: {
      ['.' as ProjectId]: {
        specifiers: { [pkgName]: version },
        dependencies,
      },
    },
    packages: packages as Record<DepPath, PackageSnapshot>,
  }
}

// This replicates the logic from @pnpm/exe's setup.js (pnpm/artifacts/exe/setup.js).
// We can't run setup.js via require() or import() because:
// - require() fails when setup.js is ESM (pnpm v11+)
// - import() is intercepted by pkg's virtual filesystem in standalone executables
// So we inline the logic: find the platform-specific binary and hard-link it
// into the @pnpm/exe package directory.
function linkExePlatformBinary (stageDir: string): void {
  const platform = process.platform === 'win32'
    ? 'win'
    : process.platform === 'darwin'
      ? 'macos'
      : process.platform
  const arch = platform === 'win' && process.arch === 'ia32' ? 'x86' : process.arch
  const executable = platform === 'win' ? 'pnpm.exe' : 'pnpm'
  const platformPkgDir = path.join(stageDir, 'node_modules', '@pnpm', `${platform}-${arch}`)
  const src = path.join(platformPkgDir, executable)
  if (!fs.existsSync(src)) return
  const exePkgDir = path.join(stageDir, 'node_modules', '@pnpm', 'exe')
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
