import assert from 'node:assert'
import fs from 'node:fs'
import path from 'node:path'

import { linkBins } from '@pnpm/bins.linker'
import { pkgRequiresBuild } from '@pnpm/building.pkg-requires-build'
import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import { calcDepState } from '@pnpm/deps.graph-hasher'
import * as dp from '@pnpm/deps.path'
import { isError, PnpmError } from '@pnpm/error'
import { runPostinstallHooks } from '@pnpm/exec.lifecycle'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import type { TarballResolution } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { logger } from '@pnpm/logger'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { pickStoreIndexKey } from '@pnpm/store.index'
import type { DepPath } from '@pnpm/types'
import { hardLinkDir } from '@pnpm/worker'
import { strict as isStrictSubdir } from 'is-subdir'

import { builtBinPackages } from './builtBinPackages.js'
import { binDirsInAllParentDirs } from './rebuildGraph.js'
import type { PackageToBuild, RebuildState } from './rebuildTypes.js'
import { relinkHoistedPackageBins } from './relinkHoistedPackageBins.js'

// Serializes builds of a shared GVS projection across concurrent per-project
// rebuilds: the first build proceeds, concurrent ones await it and reuse the
// result. Keyed by the absolute projection directory.
const gvsBuildLocks = new Map<string, Promise<void>>()

export function getPkgModulesDir (depPath: DepPath, state: RebuildState): string {
  const gvsDir = state.gvsDirByDepPath.get(depPath)
  return gvsDir != null
    ? path.join(gvsDir, 'node_modules')
    : path.join(state.ctx.virtualStoreDir, dp.depPathToFilename(depPath, state.opts.virtualStoreDirMaxLength), 'node_modules')
}

export async function runBuild (depPath: DepPath, state: RebuildState): Promise<void> {
  const pkgSnapshot = state.pkgSnapshots[depPath]
  const pkgInfo = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  const pkgRoots = getPkgRoots(depPath, pkgInfo.name, state)
  if (pkgRoots.length === 0) {
    if (pkgSnapshot.optional) return
    throw new PnpmError('MISSING_HOISTED_LOCATIONS', `${depPath} is not found in hoistedLocations inside node_modules/.modules.yaml`, {
      hint: 'If you installed your node_modules with pnpm older than v7.19.0, you may need to remove it and run "pnpm install"',
    })
  }
  const pkg: PackageToBuild = { depPath, pkgSnapshot, pkgInfo, pkgRoot: pkgRoots[0], gvsDir: state.gvsDirByDepPath.get(depPath) }
  const inFlight = pkg.gvsDir != null ? gvsBuildLocks.get(pkg.gvsDir) : undefined
  if (inFlight != null) {
    await inFlight.catch(() => {})
    state.pkgsThatWereRebuilt.add(depPath)
    return
  }
  const releaseGvsLock = pkg.gvsDir != null ? acquireGvsBuildLock(pkg.gvsDir) : undefined
  try {
    await buildPackage(pkg, state)
  } catch (err: unknown) {
    assert(isError(err))
    if (pkgSnapshot.optional) {
      await skipFailedOptionalPackage(pkg, { pkgRoots, err, state })
      return
    }
    throw err
  } finally {
    releaseGvsLock?.()
  }
  await propagateBuildOutputs(depPath, pkgRoots, state)
}

async function propagateBuildOutputs (depPath: DepPath, pkgRoots: string[], state: RebuildState): Promise<void> {
  if (pkgRoots.length > 1) {
    await hardLinkDir(pkgRoots[0], pkgRoots.slice(1))
  }
  if (state.opts.nodeLinker === 'hoisted' && state.builtDepPaths.has(depPath)) {
    await relinkHoistedPackageBins(pkgRoots, state)
  }
}

function getPkgRoots (depPath: DepPath, pkgName: string, state: RebuildState): string[] {
  const { ctx, opts } = state
  return opts.nodeLinker === 'hoisted'
    ? (ctx.modulesFile?.hoistedLocations?.[depPath] ?? []).map((hoistedLocation) => path.join(opts.lockfileDir, hoistedLocation))
    : [safeJoinModulesDir(getPkgModulesDir(depPath, state), pkgName)]
}

function acquireGvsBuildLock (gvsDir: string): () => void {
  let resolveLock!: () => void
  gvsBuildLocks.set(gvsDir, new Promise<void>((resolve) => {
    resolveLock = resolve
  }))
  return () => {
    gvsBuildLocks.delete(gvsDir)
    resolveLock()
  }
}

async function buildPackage (pkg: PackageToBuild, state: RebuildState): Promise<void> {
  const { depPath, pkgInfo } = pkg
  const extraBinPaths = await prepareBinPaths(pkg, state)
  const resolution = (pkg.pkgSnapshot.resolution as TarballResolution)
  const pkgId = pkgInfo.nonSemverVersion ?? `${pkgInfo.name}@${pkgInfo.version}`
  const cached = lookUpSideEffectsCache(pkg, { resolution, pkgId, state })
  if (cached.hit) {
    state.pkgsThatWereRebuilt.add(depPath)
    return
  }
  const hasSideEffects = await runPostinstallIfRequired(pkg, extraBinPaths, state)
  if (hasSideEffects) state.builtDepPaths.add(depPath)
  if (hasSideEffects && pkg.gvsDir != null) {
    await fs.promises.rm(path.join(pkg.pkgRoot, '.pnpm-needs-build'), { force: true })
  }
  if (hasSideEffects && (state.opts.sideEffectsCacheWrite ?? true) && (resolution.gitHosted || resolution.integrity)) {
    await uploadSideEffects(pkg, { resolution, pkgId, sideEffectsCacheKey: cached.sideEffectsCacheKey, state })
  }
  state.pkgsThatWereRebuilt.add(depPath)
}

export async function prepareBinPaths ({ depPath, pkgRoot }: Pick<PackageToBuild, 'depPath' | 'pkgRoot'>, state: RebuildState): Promise<string[]> {
  const { ctx, opts } = state
  if (opts.nodeLinker === 'hoisted') {
    return [...ctx.extraBinPaths, ...binDirsInAllParentDirs(pkgRoot, opts.lockfileDir)]
  }
  const modules = getPkgModulesDir(depPath, state)
  const binPath = path.join(pkgRoot, 'node_modules', '.bin')
  const snapshot = state.pkgSnapshots[depPath]
  const forceForPackages = builtBinPackages(modules, { ...snapshot.dependencies, ...snapshot.optionalDependencies }, state)
  await linkBins(modules, binPath, { extraNodePaths: ctx.extraNodePaths, warn: state.warn, forceForPackages })
  return ctx.extraBinPaths
}

interface SideEffectsCacheLookup {
  hit: boolean
  sideEffectsCacheKey?: string
}

function lookUpSideEffectsCache (
  { depPath, pkgRoot }: PackageToBuild,
  { resolution, pkgId, state }: { resolution: TarballResolution, pkgId: string, state: RebuildState }
): SideEffectsCacheLookup {
  const { opts } = state
  if (!opts.skipIfHasSideEffectsCache || fs.existsSync(path.join(pkgRoot, '.pnpm-needs-build')) || !(resolution.gitHosted || resolution.integrity)) {
    return { hit: false }
  }
  const filesIndexFile = pickStoreIndexKey(resolution, pkgId, { built: true })
  const pkgFilesIndex = state.storeIndex!.get(filesIndexFile) as PackageFilesIndex | undefined
  if (!pkgFilesIndex) return { hit: false }
  const sideEffectsCacheKey = calcDepState(state.depGraph, state.depsStateCache, depPath, {
    includeDepGraphHash: true,
    supportedArchitectures: opts.supportedArchitectures,
    nodeVersion: state.nodeVersion,
  })
  return { hit: pkgFilesIndex.sideEffects?.has(sideEffectsCacheKey) === true, sideEffectsCacheKey }
}

async function runPostinstallIfRequired (
  { depPath, pkgSnapshot, pkgRoot }: PackageToBuild,
  extraBinPaths: string[],
  { ctx, opts, allowBuild }: RebuildState
): Promise<boolean> {
  let requiresBuild = true
  const pgkManifest = await safeReadPackageJsonFromDir(pkgRoot)
  if (pgkManifest != null) {
    requiresBuild = pkgRequiresBuild(pgkManifest, new Map())
  }

  if (!requiresBuild || !allowBuild(depPath)) {
    return false
  }
  return runPostinstallHooks({
    depPath,
    extraBinPaths,
    extraEnv: opts.extraEnv,
    optional: pkgSnapshot.optional === true,
    pkgRoot,
    rootModulesDir: ctx.rootModulesDir,
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    shellEmulator: opts.shellEmulator,
    unsafePerm: opts.unsafePerm || false,
    userAgent: opts.userAgent,
  })
}

async function uploadSideEffects (
  { depPath, pkgRoot }: PackageToBuild,
  { resolution, pkgId, sideEffectsCacheKey, state }: {
    resolution: TarballResolution
    pkgId: string
    sideEffectsCacheKey: string | undefined
    state: RebuildState
  }
): Promise<void> {
  const filesIndexFile = pickStoreIndexKey(resolution, pkgId, { built: true })
  try {
    await state.opts.storeController.upload(pkgRoot, {
      sideEffectsCacheKey: sideEffectsCacheKey || calcDepState(state.depGraph, state.depsStateCache, depPath, {
        includeDepGraphHash: true,
        nodeVersion: state.nodeVersion,
      }),
      filesIndexFile,
    })
  } catch (err: unknown) {
    assert(isError(err))
    logger.warn({
      error: err,
      message: `An error occurred while uploading ${pkgRoot}`,
      prefix: state.opts.lockfileDir,
    })
  }
}

async function skipFailedOptionalPackage (
  { depPath, pkgSnapshot, pkgInfo }: PackageToBuild,
  { pkgRoots, err, state }: { pkgRoots: string[], err: Error, state: RebuildState }
): Promise<void> {
  const { opts } = state
  if (!state.gvsDirByDepPath.has(depPath)) {
    const rootsToRemove = opts.nodeLinker === 'hoisted'
      ? pkgRoots.filter((root) => isStrictSubdir(opts.lockfileDir, root))
      : pkgRoots
    await Promise.all(rootsToRemove.map((root) => fs.promises.rm(root, { recursive: true, force: true })))
  }
  skippedOptionalDependencyLogger.debug({
    details: err.toString(),
    package: {
      id: pkgSnapshot.id ?? depPath,
      name: pkgInfo.name,
      version: pkgInfo.version,
    },
    prefix: opts.dir,
    reason: 'build_failure',
  })
}
