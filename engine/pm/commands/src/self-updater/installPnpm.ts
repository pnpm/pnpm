import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { linkBins } from '@pnpm/bins.linker'
import { createAllowBuildFunction } from '@pnpm/building.policy'
import { getCurrentPackageName } from '@pnpm/cli.meta'
import {
  iterateHashedGraphNodes,
  iteratePkgMeta,
  lockfileToDepGraph,
} from '@pnpm/deps.graph-hasher'
import { type GlobalAddOptions, installGlobalPackages } from '@pnpm/global.commands'
import {
  cleanOrphanedInstallDirs,
  createGlobalCacheKey,
  createInstallDir,
  findGlobalPackage,
  getHashLink,
} from '@pnpm/global.packages'
import { headlessInstall } from '@pnpm/installing.deps-restorer'
import type { EnvLockfile, LockfileObject, PackageSnapshot } from '@pnpm/lockfile.types'
import { registerProject, type StoreController } from '@pnpm/store.controller'
import type { DepPath, ProjectId, ProjectRootDir, Registries } from '@pnpm/types'
import { familySync } from 'detect-libc'
import { symlinkDir } from 'symlink-dir'

// @pnpm/exe has platform-specific binaries, so its GVS hash must
// include ENGINE_NAME for correct per-platform resolution.
const PNPM_ALLOW_BUILDS: Record<string, boolean> = { '@pnpm/exe': true }

export interface InstallPnpmResult {
  binDir: string
  baseDir: string
  alreadyExisted: boolean
}

export interface InstallPnpmOptions extends GlobalAddOptions {
  envLockfile?: EnvLockfile
  storeController?: StoreController
  storeDir?: string
  packageManager?: { name: string, version: string }
}

/**
 * Installs pnpm to the global packages directory (for self-update).
 * Creates an entry in globalPkgDir that is visible to `pnpm ls -g`.
 */
export async function installPnpm (pnpmVersion: string, opts: InstallPnpmOptions): Promise<InstallPnpmResult> {
  const currentPkgName = getCurrentPackageName()

  const wantedLockfile = opts.envLockfile
    ? buildLockfileFromEnvLockfile(opts.envLockfile, currentPkgName, pnpmVersion)
    : undefined

  const result = await installPnpmToGlobalDir(
    opts,
    currentPkgName,
    pnpmVersion,
    wantedLockfile
  )

  return {
    alreadyExisted: result.alreadyExisted,
    baseDir: result.installDir,
    binDir: result.binDir,
  }
}

/**
 * Installs pnpm to the global virtual store (for version switching).
 * Does NOT create an entry in globalPkgDir — the package lives only in the store.
 * Returns the bin directory where the pnpm binary can be found.
 */
export async function installPnpmToStore (
  pnpmVersion: string,
  opts: {
    envLockfile: EnvLockfile
    storeController: StoreController
    storeDir: string
    registries: Registries
    virtualStoreDirMaxLength: number
    packageManager?: { name: string, version: string }
  }
): Promise<{ binDir: string }> {
  const currentPkgName = getCurrentPackageName()
  const wantedLockfile = buildLockfileFromEnvLockfile(opts.envLockfile, currentPkgName, pnpmVersion)
  const globalVirtualStoreDir = path.join(opts.storeDir, 'links')

  // Compute the GVS hash for the pnpm package to find its path
  const pnpmGvsPath = findPnpmGvsPath(wantedLockfile, currentPkgName, globalVirtualStoreDir, PNPM_ALLOW_BUILDS)
  const pnpmPkgDir = path.join(pnpmGvsPath, 'node_modules', currentPkgName)
  const binDir = path.join(pnpmGvsPath, 'bin')

  // Check if already installed in the GVS
  if (fs.existsSync(path.join(pnpmPkgDir, 'package.json'))) {
    if (!fs.existsSync(binDir)) {
      await linkBins(path.join(pnpmGvsPath, 'node_modules'), binDir, { warn: noop })
    }
    return { binDir }
  }

  // Install to a temporary directory — headless install with GVS enabled
  // will populate the global virtual store
  const tmpInstallDir = path.join(opts.storeDir, '.tmp', `pnpm-${pnpmVersion}-${Date.now()}`)
  fs.mkdirSync(tmpInstallDir, { recursive: true })

  try {
    await installFromLockfile(tmpInstallDir, binDir, {
      wantedLockfile,
      allowBuilds: PNPM_ALLOW_BUILDS,
      storeController: opts.storeController,
      storeDir: opts.storeDir,
      registries: opts.registries,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      packageManager: opts.packageManager,
    })

    // Now the GVS should be populated — create bins alongside the GVS entry
    linkExePlatformBinary(pnpmGvsPath)
    await linkBins(path.join(pnpmGvsPath, 'node_modules'), binDir, { warn: noop })

    return { binDir }
  } finally {
    try {
      fs.rmSync(tmpInstallDir, { recursive: true, force: true })
    } catch {}
  }
}

function noop (_message: string) {}

function findPnpmGvsPath (
  lockfile: LockfileObject,
  pkgName: string,
  globalVirtualStoreDir: string,
  allowBuilds?: Record<string, boolean | string>
): string {
  const graph = lockfileToDepGraph(lockfile)
  const pkgMetaIterator = iteratePkgMeta(lockfile, graph)
  const allowBuild = createAllowBuildFunction({ allowBuilds })
  for (const { hash, pkgMeta } of iterateHashedGraphNodes(graph, pkgMetaIterator, allowBuild)) {
    if (pkgMeta.name === pkgName) {
      return path.join(globalVirtualStoreDir, hash)
    }
  }
  throw new Error(`Could not find ${pkgName} in lockfile`)
}

interface InstallPnpmToGlobalDirResult {
  installDir: string
  binDir: string
  alreadyExisted: boolean
}

/**
 * Installs pnpm to the global packages directory.
 * Bins are created within the install dir's own bin/ subdirectory.
 *
 * When a `wantedLockfile` is provided, a frozen headless install is performed
 * using the lockfile's integrity hashes for security. Otherwise, full resolution
 * is performed via `installGlobalPackages`.
 */
async function installPnpmToGlobalDir (
  opts: InstallPnpmOptions,
  pkgName: string,
  version: string,
  wantedLockfile?: LockfileObject
): Promise<InstallPnpmToGlobalDirResult> {
  const globalDir = opts.globalPkgDir!
  cleanOrphanedInstallDirs(globalDir)

  // Check if already installed globally
  const existing = findGlobalPackage(globalDir, pkgName)
  if (existing) {
    const pkgJsonPath = path.join(existing.installDir, 'node_modules', pkgName, 'package.json')
    try {
      const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
      if (pkgJson.version === version) {
        const binDir = path.join(existing.installDir, 'bin')
        return { alreadyExisted: true, installDir: existing.installDir, binDir }
      }
    } catch {}
  }

  const installDir = createInstallDir(globalDir)
  const binDir = path.join(installDir, 'bin')

  try {
    if (wantedLockfile != null && opts.storeController != null && opts.storeDir != null) {
      await installFromLockfile(installDir, binDir, {
        wantedLockfile,
        allowBuilds: PNPM_ALLOW_BUILDS,
        storeController: opts.storeController,
        storeDir: opts.storeDir,
        registries: opts.registries as Registries,
        virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
        packageManager: opts.packageManager,
      })
      // headlessInstall does not register the project, so we must do it
      // explicitly. Without this, `pnpm store prune` would not know about
      // this install directory and would remove its packages from the
      // global virtual store.
      await registerProject(opts.storeDir, installDir)
    } else {
      await installFromResolution(installDir, opts, [`${pkgName}@${version}`])
    }

    linkExePlatformBinary(installDir)
    await linkBins(path.join(installDir, 'node_modules'), binDir, { warn: noop })

    // Create hash symlink for the global packages system
    const pkgJson = JSON.parse(fs.readFileSync(path.join(installDir, 'package.json'), 'utf8'))
    const aliases = Object.keys(pkgJson.dependencies ?? {})
    const cacheHash = createGlobalCacheKey({ aliases, registries: opts.registries })
    const hashLink = getHashLink(globalDir, cacheHash)
    await symlinkDir(installDir, hashLink, { overwrite: true })

    return { alreadyExisted: false, installDir, binDir }
  } catch (err: unknown) {
    try {
      fs.rmSync(installDir, { recursive: true, force: true })
    } catch {}
    throw err
  }
}

async function installFromLockfile (
  installDir: string,
  binDir: string,
  opts: {
    wantedLockfile: LockfileObject
    allowBuilds?: Record<string, boolean | string>
    storeController: StoreController
    storeDir: string
    registries: Registries
    virtualStoreDirMaxLength: number
    packageManager?: { name: string, version: string }
  }
): Promise<void> {
  const rootImporter = opts.wantedLockfile.importers['.' as ProjectId]
  const dependencies = rootImporter?.dependencies ?? {}
  fs.writeFileSync(path.join(installDir, 'package.json'), JSON.stringify({ dependencies }))

  await headlessInstall({
    wantedLockfile: opts.wantedLockfile,
    lockfileDir: installDir,
    storeController: opts.storeController,
    storeDir: opts.storeDir,
    registries: opts.registries,
    enableGlobalVirtualStore: true,
    globalVirtualStoreDir: path.join(opts.storeDir, 'links'),
    allowBuilds: opts.allowBuilds,
    ignoreScripts: true,
    force: false,
    engineStrict: false,
    currentEngine: {
      pnpmVersion: opts.packageManager?.version ?? '',
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
        manifest: { dependencies },
        modulesDir: path.join(installDir, 'node_modules'),
        id: '.' as ProjectId,
        rootDir: installDir as ProjectRootDir,
      },
    },
    hoistedDependencies: {},
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    sideEffectsCacheRead: false,
    sideEffectsCacheWrite: false,
    configByUri: {},
    unsafePerm: false,
    userAgent: '',
    packageManager: opts.packageManager ?? { name: 'pnpm', version: '' },
    pruneStore: false,
    pendingBuilds: [],
    skipped: new Set(),
  })
}

async function installFromResolution (
  installDir: string,
  opts: GlobalAddOptions,
  params: string[]
): Promise<void> {
  const include = {
    dependencies: true,
    devDependencies: false,
    optionalDependencies: true,
  }
  const fetchFullMetadata = Boolean(opts.supportedArchitectures?.libc)
  await installGlobalPackages({
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
    allowBuilds: {},
  }, params)
}

/**
 * Computes the scope-local directory name of the `@pnpm/exe` platform
 * package for a given host: `exe.<platform>-<arch>[-musl]`. Pure so that the
 * musl branch is unit-testable without mocking detect-libc or patching
 * process.platform.
 */
export function exePlatformPkgDirName (
  platform: NodeJS.Platform,
  arch: string,
  libcFamily: string | null
): string {
  const normalizedArch = platform === 'win32' && arch === 'ia32' ? 'x86' : arch
  const libcSuffix = platform === 'linux' && libcFamily === 'musl' ? '-musl' : ''
  return `exe.${platform}-${normalizedArch}${libcSuffix}`
}

// @pnpm/exe bundles Node.js via optional platform-specific packages
// (e.g. @pnpm/exe.darwin-arm64, @pnpm/exe.linux-x64-musl).
// Its postinstall script links the correct binary into the @pnpm/exe package dir.
// Since scripts are disabled during install (to support systems without Node.js),
// we replicate that linking here.
export function linkExePlatformBinary (installDir: string): void {
  const platform = process.platform
  const pkgDirName = exePlatformPkgDirName(platform, process.arch, familySync())
  const exePkgDir = path.join(installDir, 'node_modules', '@pnpm', 'exe')
  if (!fs.existsSync(exePkgDir)) return
  // In pnpm's symlinked node_modules layout, the platform package is not hoisted
  // to the top-level node_modules. It's a dependency of @pnpm/exe and lives as a
  // sibling in the virtual store. Resolve through the @pnpm/exe symlink to find it.
  const exeRealDir = fs.realpathSync(exePkgDir)
  const platformPkgDir = path.join(path.dirname(exeRealDir), pkgDirName)
  const executable = platform === 'win32' ? 'pnpm.exe' : 'pnpm'
  const src = path.join(platformPkgDir, executable)
  if (!fs.existsSync(src)) return
  const dest = path.join(exePkgDir, executable)
  forceLink(src, dest)

  if (platform === 'win32') {
    const exePkgJsonPath = path.join(exePkgDir, 'package.json')
    const exePkg = JSON.parse(fs.readFileSync(exePkgJsonPath, 'utf8'))
    exePkg.bin.pnpm = 'pnpm.exe'
    exePkg.bin.pn = 'pn.cmd'
    exePkg.bin.pnpx = 'pnpx.cmd'
    exePkg.bin.pnx = 'pnx.cmd'
    fs.writeFileSync(exePkgJsonPath, JSON.stringify(exePkg, null, 2))
  }
}

function forceLink (src: string, dest: string): void {
  try {
    fs.unlinkSync(dest)
  } catch (err: unknown) {
    if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'ENOENT') {
      throw err
    }
  }
  fs.linkSync(src, dest)
  fs.chmodSync(dest, 0o755)
}

function buildLockfileFromEnvLockfile (
  envLockfile: EnvLockfile,
  pkgName: string,
  version: string
) {
  const dependencies: Record<string, string> = {}
  dependencies[pkgName] = version

  const packages: Record<string, PackageSnapshot> = {}
  for (const [depPath, snapshot] of Object.entries(envLockfile.snapshots)) {
    packages[depPath as DepPath] = {
      ...snapshot,
      ...envLockfile.packages[depPath],
    }
  }

  return {
    lockfileVersion: envLockfile.lockfileVersion,
    importers: {
      ['.' as ProjectId]: {
        specifiers: { [pkgName]: version },
        dependencies,
      },
    },
    packages: packages as Record<DepPath, PackageSnapshot>,
  }
}
