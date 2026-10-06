import type { SpawnSyncReturns } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

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
  scanGlobalPackages,
} from '@pnpm/global.packages'
import { headlessInstall } from '@pnpm/installing.deps-restorer'
import type { EnvLockfile, LockfileObject, PackageSnapshot } from '@pnpm/lockfile.types'
import { registerProject, type StoreController } from '@pnpm/store.controller'
import type { DepPath, ProjectId, ProjectRootDir, RegistriesByScope } from '@pnpm/types'
import spawn from 'cross-spawn'
import { familySync } from 'detect-libc'
import semver from 'semver'
import { symlinkDir } from 'symlink-dir'

import { ensureStoreBinsLinked, linkPnpmBins } from './linkPnpmBins.js'
import { verifyPnpmEngineIdentity, type VerifyPnpmEngineIdentityOptions } from './verifyPnpmEngineIdentity.js'

export { exePlatformPkgDirName, exePlatformPkgDirNameNext, linkExePlatformBinary, nativeTargetName } from './linkExePlatformBinary.js'

// Both pnpm wrappers (`@pnpm/exe`, unscoped `pnpm`) carry platform-specific
// binaries; marking them buildable puts ENGINE_NAME in the GVS hash so each
// platform resolves to its own entry instead of colliding.
const PNPM_ALLOW_BUILDS: Record<string, boolean> = { '@pnpm/exe': true, 'pnpm': true }

/**
 * Versions whose `@pnpm/exe` shipped platform packages with no binary, so it
 * cannot run. Keyed by version, not package: the pin is shared but the wrapper
 * is not, so a developer on the JS `pnpm` — which does run at these versions —
 * would otherwise pin one and break every teammate on `@pnpm/exe`.
 */
const BROKEN_RELEASES: ReadonlySet<string> = new Set(['11.12.0', '11.13.0'])

const FIRST_PNPM_EXE_WITH_X64_MUSL_BINARY = '11.0.0-rc.3'

/**
 * Whether `version` can be installed at all — false for the
 * {@link BROKEN_RELEASES}. For callers that pick a version rather than being
 * handed one, and so can choose another instead of failing.
 */
export function isReleaseInstallable (version: string): boolean {
  return !BROKEN_RELEASES.has(version)
}

/** Throws when `version` is one of the {@link BROKEN_RELEASES}. */
export function assertReleaseIsInstallable (version: string): void {
  if (isReleaseInstallable(version)) return
  throw new PnpmError(
    'BROKEN_PNPM_RELEASE',
    `pnpm v${version} is a broken release and cannot be installed`,
    {
      hint: 'Its "@pnpm/exe" build shipped without a binary and does not run. Even where it does run, pinning it would break everyone on the project who uses "@pnpm/exe", because the pin is shared. Choose another version, or run "pnpm self-update latest".',
    }
  )
}

/**
 * Package name to install for a switch to `pnpmVersion`. From v12 the unscoped
 * `pnpm` is itself the native exe (equal content to `@pnpm/exe`), so v12+ always
 * converges on `pnpm`, even from a SEA `@pnpm/exe` build. Earlier majors keep
 * `pnpm` (JS) and `@pnpm/exe` (SEA) distinct, preserving the running identity,
 * except that `@pnpm/exe` falls back to `pnpm` where the target release has no
 * binary that runs on this host (see {@link pnpmExeRunsOn}).
 */
export function pnpmPackageNameToInstall (pnpmVersion: string, running: RunningPnpm = {}): string {
  const parsed = semver.parse(pnpmVersion, { loose: true })
  if (parsed != null && parsed.major >= 12) return 'pnpm'
  const currentPackageName = running.packageName ?? getCurrentPackageName()
  if (currentPackageName !== '@pnpm/exe') return currentPackageName
  return pnpmExeRunsOn(pnpmVersion, running.host ?? currentHost()) ? '@pnpm/exe' : 'pnpm'
}

/** Overrides for the running package and host, which default to this process. */
export interface RunningPnpm {
  packageName?: string
  host?: PnpmExeHost
}

function currentHost (): PnpmExeHost {
  return { platform: process.platform, arch: process.arch, libcFamily: familySync() }
}

export interface PnpmExeHost {
  platform: NodeJS.Platform
  arch: string
  libcFamily: string | null
}

/**
 * Whether `@pnpm/exe` of the given pre-v12 version has a working binary for
 * `host`. On musl Linux none does on arm64, as it is either missing or
 * segfaults at startup (https://github.com/pnpm/pnpm/issues/10443), and on
 * x64 the first is `11.0.0-rc.3` (https://github.com/pnpm/pnpm/issues/16467).
 * A version semver cannot parse is assumed to have one.
 */
export function pnpmExeRunsOn (pnpmVersion: string, host: PnpmExeHost): boolean {
  if (host.platform !== 'linux' || host.libcFamily !== 'musl') return true
  const parsed = semver.parse(pnpmVersion, { loose: true })
  if (parsed == null) return true
  return host.arch === 'x64' && semver.gte(parsed, FIRST_PNPM_EXE_WITH_X64_MUSL_BINARY)
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
    registriesByScope: RegistriesByScope
    virtualStoreDirMaxLength: number
    packageManager?: { name: string, version: string }
  } & VerifyPnpmEngineIdentityOptions
): Promise<{ binDir: string }> {
  const pkgName = pnpmPackageNameToInstall(pnpmVersion)
  const wantedLockfile = buildLockfileFromEnvLockfile(opts.envLockfile, pkgName, pnpmVersion)
  const globalVirtualStoreDir = path.join(opts.storeDir, 'links')

  const pnpmGvsPath = findPnpmGvsPath(wantedLockfile, pkgName, globalVirtualStoreDir, PNPM_ALLOW_BUILDS)
  const pnpmPkgDir = path.join(pnpmGvsPath, 'node_modules', pkgName)
  const binDir = path.join(pnpmGvsPath, 'bin')

  if (fs.existsSync(path.join(pnpmPkgDir, 'package.json'))) {
    await ensureStoreBinsLinked(pnpmGvsPath, binDir, pkgName)
    return { binDir }
  }

  await verifyPnpmEngineIdentity(opts.envLockfile, { name: pkgName, version: pnpmVersion }, opts)

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
      registriesByScope: opts.registriesByScope,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      packageManager: opts.packageManager,
    })

    // Now the GVS should be populated — create bins alongside the GVS entry
    await ensureStoreBinsLinked(pnpmGvsPath, binDir, pkgName)

    return { binDir }
  } finally {
    try {
      fs.rmSync(tmpInstallDir, { recursive: true, force: true })
    } catch {}
  }
}

function findPnpmGvsPath (
  lockfile: LockfileObject,
  pkgName: string,
  globalVirtualStoreDir: string,
  allowBuilds?: Record<string, boolean | string>
): string {
  const graph = lockfileToDepGraph(lockfile)
  const pkgMetaIterator = iteratePkgMeta(lockfile, graph)
  const allowBuild = createAllowBuildFunction({ allowBuilds })
  // No `lockfileDir`: this lockfile only ever holds the pnpm package and its
  // registry dependencies, and the install that materializes the slot runs from
  // a throwaway directory that differs on every self-update.
  for (const { hash, pkgMeta } of iterateHashedGraphNodes(graph, pkgMetaIterator, { allowBuild })) {
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
    await installPnpmPackage(installDir, binDir, { opts, pkgName, version, wantedLockfile })

    await linkPnpmBins(installDir, binDir, pkgName)

    // Before the caller points PNPM_HOME here, so a broken release is discarded
    // rather than swapped in.
    assertPnpmRuns(binDir, version)

    await linkGlobalHash(globalDir, installDir, opts.registriesByScope)

    return { alreadyExisted: false, installDir, binDir }
  } catch (err: unknown) {
    try {
      fs.rmSync(installDir, { recursive: true, force: true })
    } catch {}
    throw err
  }
}

interface PnpmPackageToInstall {
  opts: InstallPnpmOptions
  pkgName: string
  version: string
  wantedLockfile?: LockfileObject
}

async function installPnpmPackage (installDir: string, binDir: string, { opts, pkgName, version, wantedLockfile }: PnpmPackageToInstall): Promise<void> {
  if (wantedLockfile == null || opts.storeController == null || opts.storeDir == null) {
    await installFromResolution(installDir, opts, [`${pkgName}@${version}`])
    return
  }
  if (opts.envLockfile != null) {
    await verifyPnpmEngineIdentity(opts.envLockfile, { name: pkgName, version }, opts)
  }
  await installFromLockfile(installDir, binDir, {
    wantedLockfile,
    allowBuilds: PNPM_ALLOW_BUILDS,
    storeController: opts.storeController,
    storeDir: opts.storeDir,
    registriesByScope: opts.registriesByScope as RegistriesByScope,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    packageManager: opts.packageManager,
  })
  // headlessInstall does not register the project, so we must do it
  // explicitly. Without this, `pnpm store prune` would not know about
  // this install directory and would remove its packages from the
  // global virtual store.
  await registerProject(opts.storeDir, installDir)
}

/** Creates the hash symlink for the global packages system. */
async function linkGlobalHash (globalDir: string, installDir: string, registriesByScope: InstallPnpmOptions['registriesByScope']): Promise<void> {
  const pkgJson = JSON.parse(fs.readFileSync(path.join(installDir, 'package.json'), 'utf8'))
  const aliases = Object.keys(pkgJson.dependencies ?? {})
  const cacheHash = createGlobalCacheKey({ aliases, registriesByScope })
  const hashLink = getHashLink(globalDir, cacheHash)
  await symlinkDir(installDir, hashLink, { overwrite: true })
}

/**
 * Throws unless the pnpm CLI in `binDir` can execute — a release can install
 * cleanly and still not run, when its wrapper kept the placeholder bin of a
 * platform package that shipped without a native.
 *
 * Only that it runs is asserted; reading `--version` output would tie the check
 * to whatever startup decides to print. Exported as a test seam.
 */
export function assertPnpmRuns (binDir: string, version: string): void {
  const pnpmBinPath = path.join(binDir, 'pnpm')
  // pnpm prints its version only after loading config and switching versions,
  // so probing from the caller's directory answers with their pin rather than
  // the release under test.
  const probeDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-self-update-check-'))
  let result: SpawnSyncReturns<string>
  try {
    result = spawn.sync(pnpmBinPath, ['--version'], {
      encoding: 'utf8',
      cwd: probeDir,
    })
  } finally {
    try {
      fs.rmSync(probeDir, { recursive: true, force: true })
    } catch {}
  }
  const { status, error, stderr } = result
  if (error == null && status === 0) return
  // A signal leaves `status` null, which macOS produces for a binary its
  // signature check rejects — the exact shape of an incorrectly signed release.
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

const PNPM_ENGINE_ALIASES = new Set(['pnpm', '@pnpm/exe'])

/**
 * Unlinks every global group other than the one at `installDir` that holds
 * nothing but a pnpm engine. The engine switched from may be installed under
 * the other alias, whose group the new hash link does not overwrite
 * (https://github.com/pnpm/pnpm/issues/14709). The install directories are
 * left to `cleanOrphanedInstallDirs`, as the running pnpm may still execute
 * from one of them.
 */
export async function unlinkReplacedPnpmInstalls (globalDir: string, installDir: string): Promise<void> {
  const realInstallDir = await fs.promises.realpath(installDir)
  await Promise.all(
    scanGlobalPackages(globalDir)
      .filter((group) =>
        group.installDir !== realInstallDir &&
        Object.keys(group.dependencies).every((alias) => PNPM_ENGINE_ALIASES.has(alias))
      )
      .map((group) => fs.promises.rm(getHashLink(globalDir, group.hash), { force: true }))
  )
}

async function installFromLockfile (
  installDir: string,
  binDir: string,
  opts: {
    wantedLockfile: LockfileObject
    allowBuilds?: Record<string, boolean | string>
    storeController: StoreController
    storeDir: string
    registriesByScope: RegistriesByScope
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
    registriesByScope: opts.registriesByScope,
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
      [installDir]: createRootProject(installDir, binDir, dependencies),
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

function createRootProject (installDir: string, binDir: string, dependencies: Record<string, string>) {
  return {
    binsDir: binDir,
    buildIndex: 0,
    manifest: { dependencies },
    modulesDir: path.join(installDir, 'node_modules'),
    id: '.' as ProjectId,
    rootDir: installDir as ProjectRootDir,
  }
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
