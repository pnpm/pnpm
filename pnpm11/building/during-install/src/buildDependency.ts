import assert from 'node:assert'
import fs from 'node:fs/promises'
import path from 'node:path'

import { dirRequiresBuild } from '@pnpm/building.pkg-requires-build'
import { packageIsInstallable } from '@pnpm/config.package-is-installable'
import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import { calcDepState, type DepsStateCache } from '@pnpm/deps.graph-hasher'
import { isError, PnpmError } from '@pnpm/error'
import { runPostinstallHooks } from '@pnpm/exec.lifecycle'
import type { DirLock } from '@pnpm/fs.dir-lock'
import { logger } from '@pnpm/logger'
import { applyPatchToDir } from '@pnpm/patching.apply-patch'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { publishBuiltSharedSideEffects } from '@pnpm/pnpr.client'
import type { StoreController, UploadPkgToStoreResult } from '@pnpm/store.controller-types'
import type {
  AllowBuild,
  DepPath,
  RegistryConfig,
  RemoteSideEffectsCacheSettings,
  SupportedArchitectures,
} from '@pnpm/types'
import { hardLinkDir } from '@pnpm/worker'
import pDefer, { type DeferredPromise } from 'p-defer'

import type { DependenciesGraph, DependenciesGraphNode } from './buildGraph.js'
import { lockSlotForBuild, NEEDS_BUILD_MARKER } from './globalVirtualStoreSlot.js'
import { linkBinsOfDependencies } from './linkBinsOfDependencies.js'
import { removeIncompatibleOptional, removeSkippedOptionalDependency } from './removeSkippedDependency.js'

export interface BuildDependencyOptions {
  allowBuild: AllowBuild
  ignoredBuilds: Set<DepPath>
  extraBinPaths?: string[]
  extraNodePaths?: string[]
  extraEnv?: Record<string, string>
  depsStateCache: DepsStateCache
  ignoreScripts?: boolean
  lockfileDir: string
  optional: boolean
  preferSymlinkedExecutables?: boolean
  rootModulesDir: string
  scriptsPrependNodePath?: boolean | 'warn-only'
  scriptShell?: string
  shellEmulator?: boolean
  sideEffectsCacheWrite: boolean
  storeController: StoreController
  unsafePerm: boolean
  userAgent?: string
  hoistedLocations?: Record<string, string[]>
  builtHoistedDeps?: Record<string, DeferredPromise<void>>
  enableGlobalVirtualStore?: boolean
  frozenStore?: boolean
  configByUri?: Record<string, RegistryConfig>
  /** Resolved `engines.runtime` Node version — see [`buildModules`]. */
  nodeVersion?: string
  pnprServer?: string
  remoteSideEffectsCache?: RemoteSideEffectsCacheSettings
  supportedArchitectures?: SupportedArchitectures
  engineStrict?: boolean
  /** Node version the installability check used. Separate from the script runner. */
  engineNodeVersion?: string
  /**
   * The `node_modules` directories of the installed projects and the hoisted
   * one. A skipped optional dependency's links are removed from them.
   */
  linkedModulesDirs?: string[]
  skipped?: Set<DepPath>
  warn: (message: string) => void
}

interface BuildState {
  succeeded: boolean
  slotLock?: DirLock
}

type PatchOutcome = 'notPatched' | 'patched' | 'uninstallable'

/**
 * Whether `depPath`'s lifecycle scripts may run under the allow-build policy.
 *
 * A package the policy has no verdict on is recorded in `ignoredBuilds`, which
 * is what `pnpm approve-builds` later offers the user. An explicit `false` is
 * a decision already made, so it is not reported.
 */
export function buildIsAllowed (depPath: DepPath, allowBuild: AllowBuild, ignoredBuilds: Set<DepPath>): boolean {
  const allowed = allowBuild(depPath)
  if (allowed === undefined) {
    ignoredBuilds.add(depPath)
  }
  return allowed === true
}

export async function buildDependency<NodeId extends string> (
  depPath: NodeId,
  depGraph: DependenciesGraph<NodeId>,
  opts: BuildDependencyOptions
): Promise<void> {
  const depNode = depGraph[depPath]
  if (!depNode.filesIndexFile) return
  if (opts.builtHoistedDeps) {
    if (opts.builtHoistedDeps[depNode.depPath]) {
      await opts.builtHoistedDeps[depNode.depPath].promise
      return
    }
    opts.builtHoistedDeps[depNode.depPath] = pDefer()
  }
  const state: BuildState = { succeeded: false }
  try {
    await buildInSlot(depPath, depGraph, opts, state)
  } catch (err: unknown) {
    await skipFailedOptionalBuild(err, depNode, opts, state.slotLock)
  } finally {
    await state.slotLock?.release()
    if (state.succeeded) {
      await copyToOtherHoistedLocations(depNode, opts)
    }
    if (opts.builtHoistedDeps) {
      opts.builtHoistedDeps[depNode.depPath].resolve()
    }
  }
}

async function buildInSlot<NodeId extends string> (
  depPath: NodeId,
  depGraph: DependenciesGraph<NodeId>,
  opts: BuildDependencyOptions,
  state: BuildState
): Promise<void> {
  const depNode = depGraph[depPath]
  const slotBuild = await lockSlotIfWriting(depNode, opts)
  if (slotBuild == null) return
  state.slotLock = slotBuild.lock
  await linkBinsOfDependencies(depNode, depGraph, opts)
  const patchOutcome = await patchDependency(depPath, depGraph, opts)
  if (patchOutcome === 'uninstallable') return
  const isPatched = patchOutcome === 'patched'
  const { requiresBuild, ignoreScripts } = await gateBuildScripts(depNode, isPatched, opts)
  const hasSideEffects = !ignoreScripts && await runBuildScripts(depPath, depNode, opts)
  // A package whose build was withheld - the allow-build policy said so, or
  // ignoreScripts did - must not be cached as if it were built. The entry
  // the patch alone produced would replay on the install that finally runs
  // the build, and the scripts would never get their chance.
  const buildPending = requiresBuild && ignoreScripts
  if (!buildPending) {
    await storeBuildOutput({ depPath, depGraph, opts, isPatched, hasSideEffects })
  }
  state.succeeded = true
}

async function lockSlotIfWriting<NodeId extends string> (
  depNode: DependenciesGraphNode<NodeId>,
  opts: BuildDependencyOptions
): Promise<{ lock?: DirLock } | undefined> {
  const writesIntoSlot = opts.enableGlobalVirtualStore && (depNode.patch != null || !opts.ignoreScripts)
  if (!writesIntoSlot) return {}
  return lockSlotForBuild(depNode, opts.lockfileDir)
}

async function patchDependency<NodeId extends string> (
  depPath: NodeId,
  depGraph: DependenciesGraph<NodeId>,
  opts: BuildDependencyOptions
): Promise<PatchOutcome> {
  const depNode = depGraph[depPath]
  if (!depNode.patch) return 'notPatched'
  if (!depNode.patch.patchFilePath) {
    throw new PnpmError('PATCH_FILE_PATH_MISSING',
      `Cannot apply patch for ${depPath}: patch file path is missing`,
      { hint: 'Ensure the package is listed in patchedDependencies configuration' }
    )
  }
  const isPatched = applyPatchToDir({ patchedDir: depNode.dir, patchFilePath: depNode.patch.patchFilePath })
  if (!isPatched) return 'notPatched'
  if (!opts.engineStrict || await patchedPackageIsInstallable(depPath, depNode, opts)) return 'patched'
  await removeIncompatibleOptional(depPath, depNode, depGraph, opts)
  return 'uninstallable'
}

async function patchedPackageIsInstallable<NodeId extends string> (
  depPath: NodeId,
  depNode: DependenciesGraphNode<NodeId>,
  opts: BuildDependencyOptions
): Promise<boolean> {
  const patched = await safeReadPackageJsonFromDir(depNode.dir)
  if (patched == null) {
    throw new PnpmError(
      'PATCHED_MANIFEST_UNREADABLE',
      `Cannot read the patched package.json of ${depPath}`
    )
  }
  const installable = packageIsInstallable(depPath, {
    name: patched.name ?? '',
    version: patched.version ?? '0.0.0',
    engines: patched.engines,
  }, {
    engineStrict: !depNode.optional,
    lockfileDir: opts.lockfileDir,
    nodeVersion: opts.engineNodeVersion ?? opts.nodeVersion,
    optional: depNode.optional,
    supportedArchitectures: opts.supportedArchitectures,
  })
  return installable !== false
}

/**
 * A patch can add install scripts - or a binding.gyp, which the lifecycle
 * runner turns into `node-gyp rebuild` - to a package that published
 * neither, and the caller's gate could not have seen that: the files it
 * read requiresBuild off were still unpatched. Build work a patch
 * introduces needs approval like any other, so put it through the same gate.
 * The recompute runs even when scripts are already suppressed, because
 * the caller needs to know a build is owed either way.
 */
async function gateBuildScripts<NodeId extends string> (
  depNode: DependenciesGraphNode<NodeId>,
  isPatched: boolean,
  opts: BuildDependencyOptions
): Promise<{ requiresBuild: boolean, ignoreScripts: boolean }> {
  const requiresBuild = depNode.requiresBuild === true
  const ignoreScripts = Boolean(opts.ignoreScripts)
  if (!isPatched || requiresBuild) return { requiresBuild, ignoreScripts }
  const patchRequiresBuild = await dirRequiresBuild(depNode.dir)
  return {
    requiresBuild: patchRequiresBuild,
    ignoreScripts: ignoreScripts || (patchRequiresBuild && !buildIsAllowed(depNode.depPath, opts.allowBuild, opts.ignoredBuilds)),
  }
}

async function runBuildScripts<NodeId extends string> (
  depPath: NodeId,
  depNode: DependenciesGraphNode<NodeId>,
  opts: BuildDependencyOptions
): Promise<boolean> {
  return runPostinstallHooks({
    depPath,
    extraBinPaths: opts.extraBinPaths,
    extraEnv: opts.extraEnv,
    initCwd: opts.lockfileDir,
    optional: depNode.optional,
    pkgRoot: depNode.dir,
    rootModulesDir: opts.rootModulesDir,
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    scriptShell: opts.scriptShell,
    shellEmulator: opts.shellEmulator,
    unsafePerm: opts.unsafePerm || false,
    userAgent: opts.userAgent,
  })
}

interface BuildOutput<NodeId extends string> {
  depPath: NodeId
  depGraph: DependenciesGraph<NodeId>
  opts: BuildDependencyOptions
  isPatched: boolean
  hasSideEffects: boolean
}

async function storeBuildOutput<NodeId extends string> (output: BuildOutput<NodeId>): Promise<void> {
  const { depPath, depGraph, opts, isPatched, hasSideEffects } = output
  const depNode = depGraph[depPath]
  // Remove the .pnpm-needs-build marker before uploading side effects,
  // so it doesn't get cached as part of the package's side effects diff.
  // A withheld build keeps it, so the install that runs the build finds it.
  if (opts.enableGlobalVirtualStore) {
    await fs.unlink(path.join(depNode.dir, NEEDS_BUILD_MARKER)).catch(() => {})
  }
  // frozenStore opens the store read-only, so the side-effects cache (which
  // lives in the store) cannot be written. extendInstallOptions already forces
  // sideEffectsCacheWrite off under frozenStore; this guards callers that
  // bypass it.
  const shouldPublishSharedSideEffects = hasSideEffects && sharesSideEffectsOf(depNode, opts)
  if (!isPatched && !hasSideEffects) return
  if (!(opts.sideEffectsCacheWrite || shouldPublishSharedSideEffects) || opts.frozenStore) return
  try {
    await uploadBuildOutput(output, shouldPublishSharedSideEffects)
  } catch (err: unknown) {
    assert(isError(err))
    logger.warn({
      error: err,
      message: `An error occurred while uploading ${depNode.dir}`,
      prefix: opts.lockfileDir,
    })
  }
}

async function uploadBuildOutput<NodeId extends string> (
  output: BuildOutput<NodeId>,
  shouldPublishSharedSideEffects: boolean
): Promise<void> {
  const { depPath, depGraph, opts, hasSideEffects } = output
  const depNode = depGraph[depPath]
  const sideEffectsCacheKey = calcDepState(depGraph, opts.depsStateCache, depPath, {
    patchFileHash: depNode.patch?.hash,
    includeDepGraphHash: hasSideEffects,
    nodeVersion: opts.nodeVersion,
  })
  if (!depNode.filesIndexFile) return
  const upload = await opts.storeController.upload(depNode.dir, {
    sideEffectsCacheKey,
    filesIndexFile: depNode.filesIndexFile,
  })
  if (shouldPublishSharedSideEffects && depNode.resolution != null) {
    await publishSideEffects({ depPath, depGraph, opts, resolution: depNode.resolution, upload })
  }
}

function sharesSideEffectsOf<NodeId extends string> (depNode: DependenciesGraphNode<NodeId>, opts: BuildDependencyOptions): boolean {
  return opts.remoteSideEffectsCache?.publish === true &&
    opts.pnprServer != null &&
    opts.remoteSideEffectsCache?.packages?.includes(depNode.name) === true &&
    depNode.resolution != null
}

async function publishSideEffects<NodeId extends string> (
  { depPath, depGraph, opts, resolution, upload }: Pick<BuildOutput<NodeId>, 'depPath' | 'depGraph' | 'opts'> & {
    resolution: NonNullable<DependenciesGraphNode<NodeId>['resolution']>
    upload: UploadPkgToStoreResult
  }
): Promise<void> {
  const depNode = depGraph[depPath]
  await publishBuiltSharedSideEffects({
    configByUri: opts.configByUri ?? {},
    depsGraph: depGraph,
    graphKey: depPath,
    name: depNode.name,
    nodeVersion: opts.nodeVersion,
    patchFileHash: depNode.patch?.hash,
    pnprServer: opts.pnprServer,
    resolution,
    settings: opts.remoteSideEffectsCache,
    supportedArchitectures: opts.supportedArchitectures,
    upload,
    version: depNode.version,
  })
}

async function skipFailedOptionalBuild<NodeId extends string> (
  err: unknown,
  depNode: DependenciesGraphNode<NodeId>,
  opts: BuildDependencyOptions,
  slotLock: DirLock | undefined
): Promise<void> {
  assert(isError(err))
  if (!depNode.optional) throw err
  // Without the lock another install may be writing into the shared
  // slot, so the slot is kept, marked for the next install to rebuild.
  if (!opts.enableGlobalVirtualStore || slotLock != null) {
    await removeSkippedOptionalDependency(depNode, opts)
  }
  // TODO: add parents field to the log
  skippedOptionalDependencyLogger.debug({
    details: err.toString(),
    package: {
      id: depNode.dir,
      name: depNode.name,
      version: depNode.version,
    },
    prefix: opts.lockfileDir,
    reason: 'build_failure',
  })
}

async function copyToOtherHoistedLocations<NodeId extends string> (
  depNode: DependenciesGraphNode<NodeId>,
  opts: BuildDependencyOptions
): Promise<void> {
  const hoistedLocationsOfDep = opts.hoistedLocations?.[depNode.depPath]
  if (!hoistedLocationsOfDep) return
  // There is no need to build the same package in every location.
  // We just copy the built package to every location where it is present.
  const currentHoistedLocation = path.relative(opts.lockfileDir, depNode.dir)
  // The destinations must be resolved here, on the main thread: hardLinkDir()
  // runs on a worker thread, and applyPatchToDir() switches the process-wide
  // cwd, so a worker resolving a relative path inside that window would
  // resolve it against the wrong directory.
  const nonBuiltHoistedDeps = hoistedLocationsOfDep.filter((hoistedLocation) => hoistedLocation !== currentHoistedLocation)
    .map((hoistedLocation) => path.join(opts.lockfileDir, hoistedLocation))
  await hardLinkDir(depNode.dir, nonBuiltHoistedDeps)
}
