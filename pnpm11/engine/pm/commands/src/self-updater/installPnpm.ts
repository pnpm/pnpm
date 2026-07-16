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
import { PnpmError } from '@pnpm/error'
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
import spawn from 'cross-spawn'
import { familySync } from 'detect-libc'
import semver from 'semver'
import { symlinkDir } from 'symlink-dir'

import { verifyPnpmEngineIdentity, type VerifyPnpmEngineIdentityOptions } from './verifyPnpmEngineIdentity.js'

// Both pnpm wrappers (`@pnpm/exe`, unscoped `pnpm`) carry platform-specific
// binaries; marking them buildable puts ENGINE_NAME in the GVS hash so each
// platform resolves to its own entry instead of colliding.
const PNPM_ALLOW_BUILDS: Record<string, boolean> = { '@pnpm/exe': true, 'pnpm': true }

/**
 * Package name to install for a switch to `pnpmVersion`. From v12 the unscoped
 * `pnpm` is itself the native exe (equal content to `@pnpm/exe`), so v12+ always
 * converges on `pnpm`, even from a SEA `@pnpm/exe` build. Earlier majors keep
 * `pnpm` (JS) and `@pnpm/exe` (SEA) distinct, preserving the running identity.
 */
export function pnpmPackageNameToInstall (pnpmVersion: string): string {
  const parsed = semver.parse(pnpmVersion, { loose: true })
  if (parsed != null && parsed.major >= 12) return 'pnpm'
  return getCurrentPackageName()
}

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
  /** See {@link VerifyPnpmEngineIdentityOptions.trustedKeys} — a test seam. */
  trustedKeys?: VerifyPnpmEngineIdentityOptions['trustedKeys']
}

/**
 * Installs pnpm to the global packages directory (for self-update).
 * Creates an entry in globalPkgDir that is visible to `pnpm ls -g`.
 */
export async function installPnpm (pnpmVersion: string, opts: InstallPnpmOptions): Promise<InstallPnpmResult> {
  const pkgName = pnpmPackageNameToInstall(pnpmVersion)

  const wantedLockfile = opts.envLockfile
    ? buildLockfileFromEnvLockfile(opts.envLockfile, pkgName, pnpmVersion)
    : undefined

  const result = await installPnpmToGlobalDir(
    opts,
    pkgName,
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
  } & VerifyPnpmEngineIdentityOptions
): Promise<{ binDir: string }> {
  const pkgName = pnpmPackageNameToInstall(pnpmVersion)
  const wantedLockfile = buildLockfileFromEnvLockfile(opts.envLockfile, pkgName, pnpmVersion)
  const globalVirtualStoreDir = path.join(opts.storeDir, 'links')

  // Compute the GVS hash for the pnpm package to find its path
  const pnpmGvsPath = findPnpmGvsPath(wantedLockfile, pkgName, globalVirtualStoreDir, PNPM_ALLOW_BUILDS)
  const pnpmPkgDir = path.join(pnpmGvsPath, 'node_modules', pkgName)
  const binDir = path.join(pnpmGvsPath, 'bin')

  // Check if already installed in the GVS
  if (fs.existsSync(path.join(pnpmPkgDir, 'package.json'))) {
    if (!fs.existsSync(binDir)) {
      await linkBins(path.join(pnpmGvsPath, 'node_modules'), binDir, { warn: noop })
    }
    return { binDir }
  }

  // Reached only on a store cache miss (a genuine download), so verifying the
  // pnpm engine's registry signature here does not slow down repeated commands.
  await verifyPnpmEngineIdentity(opts.envLockfile, pnpmVersion, opts)

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
    linkExePlatformBinary(pnpmGvsPath, pkgName)
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

  const existingInstallDir = await findGlobalPnpmInstallDir(globalDir, pkgName, version)
  if (existingInstallDir != null) {
    return { alreadyExisted: true, installDir: existingInstallDir, binDir: path.join(existingInstallDir, 'bin') }
  }

  const installDir = createInstallDir(globalDir)
  const binDir = path.join(installDir, 'bin')

  try {
    if (wantedLockfile != null && opts.storeController != null && opts.storeDir != null) {
      if (opts.envLockfile != null) {
        // Reached only when actually downloading (no matching global install),
        // so the signature check does not run on every invocation.
        await verifyPnpmEngineIdentity(opts.envLockfile, version, opts)
      }
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

    linkExePlatformBinary(installDir, pkgName)
    await linkBins(path.join(installDir, 'node_modules'), binDir, { warn: noop })

    // Nothing above proves the installed CLI can actually run: a wrapper whose
    // platform package shipped without its native keeps the placeholder bin
    // from the tarball, and a truncated or mis-signed binary is equally silent.
    // Catching that here — before the caller points PNPM_HOME at this dir — is
    // what keeps a bad release from replacing a working pnpm, and the catch
    // below removes the directory so the next run reinstalls it.
    assertPnpmRuns(binDir, version)

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

/**
 * Throws unless the pnpm CLI in `binDir` can execute.
 *
 * Only that it runs is asserted, not what it prints: the point is to reject an
 * executable that cannot start at all, and matching `--version` output exactly
 * would fail on anything else a release chooses to write to stdout.
 *
 * Spawned through cross-spawn, and by the same bare `pnpm` name the version
 * switcher spawns, so that the `.cmd` shim `linkBins` writes on Windows is
 * resolved rather than executed as a script. Exported as a test seam, since
 * reaching it through an install would mean publishing a deliberately broken
 * pnpm tarball as a fixture.
 */
export function assertPnpmRuns (binDir: string, version: string): void {
  const pnpmBinPath = path.join(binDir, 'pnpm')
  const { status, error, stderr } = spawn.sync(pnpmBinPath, ['--version'], { encoding: 'utf8' })
  if (error == null && status === 0) return
  // A signal leaves `status` null, which macOS produces for a binary its
  // signature check rejects — the exact shape of a mis-signed release.
  const exit = status != null ? `code ${status}` : 'a signal'
  const reason = error != null
    ? error.message
    : `it exited with ${exit}${(stderr ?? '').trim() ? `: ${stderr.trim()}` : ''}`
  throw new PnpmError(
    'BROKEN_PNPM_INSTALL',
    `The pnpm v${version} that was just installed cannot run: ${reason}`,
    {
      hint: `The installation at "${pnpmBinPath}" was discarded and the currently active pnpm was left in place, so pnpm still works. A release that installs but cannot run is a packaging fault — please report it at https://github.com/pnpm/pnpm/issues. To move to a different version meanwhile, pass one to "pnpm self-update".`,
    }
  )
}

/**
 * The install dir under `globalDir` that already holds `pkgName` at exactly
 * `version`, or `undefined` when the global install is missing, at a
 * different version, or unreadable.
 */
export async function findGlobalPnpmInstallDir (globalDir: string, pkgName: string, version: string): Promise<string | undefined> {
  const existing = findGlobalPackage(globalDir, pkgName)
  if (!existing) return undefined
  try {
    const pkgJson = JSON.parse(await fs.promises.readFile(path.join(existing.installDir, 'node_modules', pkgName, 'package.json'), 'utf8'))
    if (pkgJson.version === version) return existing.installDir
  } catch {}
  return undefined
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
    include,
    includeDirect: include,
    allowBuilds: {},
  }, params)
}

/**
 * Computes the scope-local directory name of the `@pnpm/exe` platform
 * package for a given host. Returns the legacy name currently published on npm
 * (`macos-<arch>`, `win-<arch>`, `linux-<arch>`, `linuxstatic-<arch>`); callers
 * should also consider the future `exe.<platform>-<arch>[-musl]` scheme, since
 * a later release will switch to it. Pure so that the musl branch is
 * unit-testable without mocking detect-libc or patching process.platform.
 */
export function exePlatformPkgDirName (
  platform: NodeJS.Platform,
  arch: string,
  libcFamily: string | null
): string {
  const normalizedArch = platform === 'win32' && arch === 'ia32' ? 'x86' : arch
  return `${legacyOsSegment(platform, libcFamily)}-${normalizedArch}`
}

function legacyOsSegment (platform: NodeJS.Platform, libcFamily: string | null): string {
  switch (platform) {
    case 'darwin': return 'macos'
    case 'win32': return 'win'
    case 'linux': return libcFamily === 'musl' ? 'linuxstatic' : 'linux'
    default: return platform
  }
}

/**
 * Scope-local directory name of the platform package under the
 * `exe.<platform>-<arch>[-musl]` scheme, i.e. the published package
 * `@pnpm/exe.<platform>-<arch>[-musl]`. pnpm v12 (the Rust port) ships its
 * native binaries under exactly this convention, so `linkExePlatformBinary`
 * relinks a v12 install with no v12-specific logic. `@pnpm/exe` (the
 * TypeScript SEA build) is expected to adopt the same scheme in a future
 * release, which is why the legacy `@pnpm/<os>-<arch>` name is still checked
 * first as a fallback.
 */
export function exePlatformPkgDirNameNext (
  platform: NodeJS.Platform,
  arch: string,
  libcFamily: string | null
): string {
  const normalizedArch = platform === 'win32' && arch === 'ia32' ? 'x86' : arch
  const libcSuffix = platform === 'linux' && libcFamily === 'musl' ? '-musl' : ''
  return `exe.${platform}-${normalizedArch}${libcSuffix}`
}

// The wrapper's preinstall links the platform binary into the wrapper dir, but
// scripts are disabled during pnpm's own installs, so replicate it here — trying
// the legacy and the newer `exe.<target>` platform-package names.
export function linkExePlatformBinary (installDir: string, wrapperPkgName: string = '@pnpm/exe'): void {
  const wrapperDir = path.join(installDir, 'node_modules', ...wrapperPkgName.split('/'))
  if (!fs.existsSync(wrapperDir)) return
  const platform = process.platform
  const arch = process.arch
  const libcFamily = familySync()
  const executable = platform === 'win32' ? 'pnpm.exe' : 'pnpm'
  const wrapperRealDir = fs.realpathSync(wrapperDir)
  const adjacentScopeDir = wrapperPkgName.startsWith('@')
    ? path.dirname(wrapperRealDir)
    : path.join(path.dirname(wrapperRealDir), '@pnpm')
  // GVS dependencies link to sibling slots through the install root. The real
  // adjacent scope remains necessary for legacy virtual-store layouts.
  const scopeDirs = new Set([
    adjacentScopeDir,
    path.join(installDir, 'node_modules', '@pnpm'),
  ])
  const candidateDirNames = [
    exePlatformPkgDirName(platform, arch, libcFamily),
    exePlatformPkgDirNameNext(platform, arch, libcFamily),
  ]
  let src: string | undefined
  for (const scopeDir of scopeDirs) {
    for (const dirName of candidateDirNames) {
      const candidate = path.join(scopeDir, dirName, executable)
      if (fs.existsSync(candidate)) {
        src = candidate
        break
      }
    }
    if (src != null) break
  }
  if (src == null) return
  const dest = path.join(wrapperDir, executable)
  forceLink(src, dest)

  if (platform === 'win32') {
    // Aliases (pn / pnpx / pnx) need to be .exe hardlinks of the native binary,
    // not the .cmd wrappers we ship in the tarball. cmd-shim's Bash shim for
    // a .cmd target wraps it in `exec cmd /C ...`, and MSYS2 / Git Bash
    // mangles `/C` into a Windows path — cmd.exe then falls into interactive
    // mode and prints its banner instead of running the alias. .exe sources
    // sidestep cmd-shim's wrapper. The native binary detects which name it was
    // launched as via process.execPath and prepends `dlx` for pnpx / pnx.
    // See https://github.com/pnpm/pnpm/issues/11486.
    for (const alias of ['pn', 'pnpx', 'pnx']) {
      forceLink(src, path.join(wrapperDir, `${alias}.exe`))
    }

    const wrapperPkgJsonPath = path.join(wrapperDir, 'package.json')
    const wrapperPkg = JSON.parse(fs.readFileSync(wrapperPkgJsonPath, 'utf8'))
    wrapperPkg.bin.pnpm = 'pnpm.exe'
    wrapperPkg.bin.pn = 'pn.exe'
    wrapperPkg.bin.pnpx = 'pnpx.exe'
    wrapperPkg.bin.pnx = 'pnx.exe'
    // Temp file + rename, not in-place: package.json is hard-linked from the
    // content-addressable store, so writing in place would mutate the shared blob.
    const tempPkgJsonPath = `${wrapperPkgJsonPath}.pnpm-tmp`
    try {
      fs.writeFileSync(tempPkgJsonPath, JSON.stringify(wrapperPkg, null, 2))
      fs.renameSync(tempPkgJsonPath, wrapperPkgJsonPath)
    } catch (err: unknown) {
      try {
        fs.rmSync(tempPkgJsonPath, { force: true })
      } catch {}
      throw err
    }
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
