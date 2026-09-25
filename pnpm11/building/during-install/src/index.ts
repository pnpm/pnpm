import assert from 'node:assert'
import fs from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

import { linkBins, linkBinsOfPackages } from '@pnpm/bins.linker'
import { dirRequiresBuild } from '@pnpm/building.pkg-requires-build'
import { getWorkspaceConcurrency } from '@pnpm/config.reader'
import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import { calcDepState, type DepsStateCache, findRuntimeNodeVersion } from '@pnpm/deps.graph-hasher'
import { isRuntimeDepPath } from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import { runPostinstallHooks } from '@pnpm/exec.lifecycle'
import { DirLock } from '@pnpm/fs.dir-lock'
import { logger } from '@pnpm/logger'
import { applyPatchToDir } from '@pnpm/patching.apply-patch'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { publishBuiltSharedSideEffects } from '@pnpm/pnpr.client'
import type { StoreController } from '@pnpm/store.controller-types'
import type {
  AllowBuild,
  DependencyManifest,
  DepPath,
  IgnoredBuilds,
  RegistryConfig,
  RemoteSideEffectsCacheSettings,
  SupportedArchitectures,
} from '@pnpm/types'
import { hardLinkDir } from '@pnpm/worker'
import { scheduleGraph, type TaskCompletion } from '@pnpm/workspace.task-scheduler'
import pDefer, { type DeferredPromise } from 'p-defer'
import { pathExists } from 'path-exists'
import { pickBy } from 'ramda'

import { buildGraph, type DependenciesGraph, type DependenciesGraphNode } from './buildGraph.js'

export type { DepsStateCache }

const NEEDS_BUILD_MARKER = '.pnpm-needs-build'
const STARTED_BUILD_MARKER_CONTENT = 'started'
const SLOT_LOCK_DIR = '.pnpm-build.lock'
const SLOT_LOCK_WAIT_MS = 10 * 60_000
// Comfortably above how long a build can take, so a live holder never has its lock stolen mid-build.
const SLOT_LOCK_ABANDONED_MS = 30 * 60_000

export async function buildModules<T extends string> (
  depGraph: DependenciesGraph<T>,
  rootDepPaths: T[],
  opts: {
    allowBuild?: AllowBuild
    childConcurrency?: number
    depsToBuild?: Set<string>
    depsStateCache: DepsStateCache
    extraBinPaths?: string[]
    extraNodePaths?: string[]
    extraEnv?: Record<string, string>
    ignoreScripts?: boolean
    lockfileDir: string
    optional: boolean
    preferSymlinkedExecutables?: boolean
    unsafePerm: boolean
    userAgent: string
    scriptsPrependNodePath?: boolean | 'warn-only'
    scriptShell?: string
    shellEmulator?: boolean
    sideEffectsCacheWrite: boolean
    storeController: StoreController
    rootModulesDir: string
    hoistedLocations?: Record<string, string[]>
    enableGlobalVirtualStore?: boolean
    frozenStore?: boolean
    configByUri?: Record<string, RegistryConfig>
    pnprServer?: string
    remoteSideEffectsCache?: RemoteSideEffectsCacheSettings
    supportedArchitectures?: SupportedArchitectures
  }
): Promise<{ ignoredBuilds?: IgnoredBuilds }> {
  if (!rootDepPaths.length) return {}
  const warn = (message: string) => {
    logger.warn({ message, prefix: opts.lockfileDir })
  }
  // postinstall hooks

  // Resolved `engines.runtime` Node version (when the project pins
  // one) so each per-snapshot side-effects-cache key reflects the
  // script-runner Node. Computed once over the install-wide graph
  // and threaded into [`buildDependency`] via [`buildDepOpts`].
  const nodeVersion = findRuntimeNodeVersion(Object.keys(depGraph))
  const buildDepOpts = {
    ...opts,
    builtHoistedDeps: opts.hoistedLocations ? {} : undefined,
    nodeVersion,
    warn,
  }
  const dependencyGraph = buildGraph<T>(depGraph, rootDepPaths)
  if (dependencyGraph.size === 0) return {}
  const ignoredBuilds = new Set<DepPath>()
  const allowBuild = opts.allowBuild ?? (() => undefined)
  // Under the global virtual store a package's directory lives inside the store
  // (`{storeDir}/links/...`), so applying a patch or running an allowlisted
  // lifecycle script writes into it. On a read-only `frozenStore` that write
  // would crash mid-build with a raw `EROFS`. A complete seed never reaches the
  // build step — built and patched packages are imported from the side-effects
  // cache with `isBuilt` set and filtered out just below — so any package still
  // wanting to write means the seed is missing its build output. We collect
  // those off the same filtered graph and refuse up front (see
  // `throwFrozenStoreNeedsBuild`) instead of failing cryptically once a script
  // starts. Bin-linking reuses existing symlinks write-free, and non-allowlisted
  // scripts never run, so neither counts as a blocking write. Optional
  // dependencies don't block either — their build failures are non-fatal at
  // runtime, so their builds are skipped instead.
  const frozenStoreBlocked = (opts.frozenStore && opts.enableGlobalVirtualStore)
    ? new Set<string>()
    : undefined
  if (frozenStoreBlocked != null) {
    for (const depPath of dependencyGraph.keys()) {
      if (shouldBuild(depPath)) {
        const node = depGraph[depPath]
        // A patch is applied even under `ignoreScripts`, but a lifecycle script
        // is not — so only the patch write counts as blocking when scripts are
        // suppressed.
        const willPatch = node.patch != null
        const willRunScripts = !opts.ignoreScripts && Boolean(node.requiresBuild) && allowBuild(node.depPath) === true
        if (!willPatch && !willRunScripts) continue
        if (node.optional) {
          // A build/patch failure on an optional dependency is non-fatal at
          // runtime (see the catch in `buildDependency`), so a seed missing an
          // optional package's build output skips that build instead of
          // blocking the install.
          skippedOptionalDependencyLogger.debug({
            details: `The read-only store (frozenStore) is missing the build output of ${node.name}@${node.version}.`,
            package: {
              id: node.dir,
              name: node.name,
              version: node.version,
            },
            prefix: opts.lockfileDir,
            reason: 'build_failure',
          })
          continue
        }
        frozenStoreBlocked.add(`${node.name}@${node.version}`)
      }
    }
  }
  if (frozenStoreBlocked?.size) {
    throwFrozenStoreNeedsBuild(frozenStoreBlocked)
  }
  const patchErrors: Error[] = []
  let firstError: unknown
  await scheduleGraph(dependencyGraph, {
    bail: true,
    concurrency: getWorkspaceConcurrency(opts.childConcurrency),
    runNode: async (depPath): Promise<TaskCompletion> => {
      if (!shouldBuild(depPath)) return 'passed'
      try {
        const node = depGraph[depPath]
        const ignoreScripts = Boolean(buildDepOpts.ignoreScripts) ||
          (Boolean(node.requiresBuild) && !buildIsAllowed(node.depPath, allowBuild, ignoredBuilds))
        await buildDependency(depPath, depGraph, {
          ...buildDepOpts,
          allowBuild,
          ignoredBuilds,
          ignoreScripts,
        })
        return 'passed'
      } catch (err: unknown) {
        if (util.types.isNativeError(err) && 'code' in err && err.code === 'ERR_PNPM_PATCH_FAILED') {
          patchErrors.push(err)
          return 'passed'
        }
        firstError ??= err
        return 'aborted'
      }
    },
    onNodeSkipped: () => {},
  })
  if (firstError != null) throw firstError
  if (patchErrors.length > 0) {
    throw patchErrors[0]
  }
  return { ignoredBuilds }

  function shouldBuild (depPath: T): boolean {
    const node = depGraph[depPath]
    return (node.requiresBuild || node.patch != null) && !node.isBuilt &&
      (opts.depsToBuild == null || opts.depsToBuild.has(depPath))
  }
}

/**
 * Whether `depPath`'s lifecycle scripts may run under the allow-build policy.
 *
 * A package the policy has no verdict on is recorded in `ignoredBuilds`, which
 * is what `pnpm approve-builds` later offers the user. An explicit `false` is
 * a decision already made, so it is not reported.
 */
function buildIsAllowed (depPath: DepPath, allowBuild: AllowBuild, ignoredBuilds: Set<DepPath>): boolean {
  const allowed = allowBuild(depPath)
  if (allowed === undefined) {
    ignoredBuilds.add(depPath)
  }
  return allowed === true
}

/** Refuse a build under a read-only global virtual store. See the call site. */
function throwFrozenStoreNeedsBuild (blocked: Set<string>): never {
  const list = Array.from(blocked).sort()
  throw new PnpmError(
    'FROZEN_STORE_NEEDS_BUILD',
    `Cannot build the following ${list.length === 1 ? 'package' : 'packages'} because the store is read-only (frozenStore is enabled): ${list.join(', ')}`,
    {
      hint: 'This read-only store was not seeded with these packages\' build output. Rebuild the seed with their scripts enabled so the side-effects cache is populated, or remove them from onlyBuiltDependencies.',
    }
  )
}

async function buildDependency<T extends string> (
  depPath: T,
  depGraph: DependenciesGraph<T>,
  opts: {
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
    warn: (message: string) => void
  }
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
  let buildSucceeded = false
  let slotLock: DirLock | undefined
  try {
    if (opts.enableGlobalVirtualStore && (depNode.patch != null || !opts.ignoreScripts)) {
      const slotBuild = await lockSlotForBuild(depNode, opts.lockfileDir)
      if (slotBuild == null) return
      slotLock = slotBuild.lock
    }
    await linkBinsOfDependencies(depNode, depGraph, opts)
    let isPatched = false
    if (depNode.patch) {
      if (!depNode.patch.patchFilePath) {
        throw new PnpmError('PATCH_FILE_PATH_MISSING',
          `Cannot apply patch for ${depPath}: patch file path is missing`,
          { hint: 'Ensure the package is listed in patchedDependencies configuration' }
        )
      }
      isPatched = applyPatchToDir({ patchedDir: depNode.dir, patchFilePath: depNode.patch.patchFilePath })
    }
    // A patch can add install scripts - or a binding.gyp, which the lifecycle
    // runner turns into `node-gyp rebuild` - to a package that published
    // neither, and the caller's gate could not have seen that: the files it
    // read requiresBuild off were still unpatched. Build work a patch
    // introduces needs approval like any other, so put it through the same gate.
    // The recompute runs even when scripts are already suppressed, because
    // `buildPending` below needs to know a build is owed either way.
    let requiresBuild = depNode.requiresBuild === true
    let ignoreScripts = Boolean(opts.ignoreScripts)
    if (isPatched && !requiresBuild) {
      requiresBuild = await dirRequiresBuild(depNode.dir)
      if (requiresBuild && !ignoreScripts) {
        ignoreScripts = !buildIsAllowed(depNode.depPath, opts.allowBuild, opts.ignoredBuilds)
      }
    }
    const hasSideEffects = !ignoreScripts && await runPostinstallHooks({
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
    // A package whose build was withheld - the allow-build policy said so, or
    // ignoreScripts did - must not be cached as if it were built. The entry
    // the patch alone produced would replay on the install that finally runs
    // the build, and the scripts would never get their chance.
    const buildPending = requiresBuild && ignoreScripts
    // Remove the .pnpm-needs-build marker before uploading side effects,
    // so it doesn't get cached as part of the package's side effects diff.
    // A withheld build keeps it, so the install that runs the build finds it.
    if (opts.enableGlobalVirtualStore && !buildPending) {
      await fs.unlink(path.join(depNode.dir, NEEDS_BUILD_MARKER)).catch(() => {})
    }
    // frozenStore opens the store read-only, so the side-effects cache (which
    // lives in the store) cannot be written. extendInstallOptions already forces
    // sideEffectsCacheWrite off under frozenStore; this guards callers that
    // bypass it.
    const shouldPublishSharedSideEffects = hasSideEffects &&
      opts.remoteSideEffectsCache?.publish === true &&
      opts.pnprServer != null &&
      opts.remoteSideEffectsCache?.packages?.includes(depNode.name) === true &&
      depNode.resolution != null
    if ((isPatched || hasSideEffects) && !buildPending && (opts.sideEffectsCacheWrite || shouldPublishSharedSideEffects) && !opts.frozenStore) {
      try {
        const sideEffectsCacheKey = calcDepState(depGraph, opts.depsStateCache, depPath, {
          patchFileHash: depNode.patch?.hash,
          includeDepGraphHash: hasSideEffects,
          nodeVersion: opts.nodeVersion,
        })
        const upload = await opts.storeController.upload(depNode.dir, {
          sideEffectsCacheKey,
          filesIndexFile: depNode.filesIndexFile,
        })
        if (shouldPublishSharedSideEffects && depNode.resolution != null) {
          await publishBuiltSharedSideEffects({
            configByUri: opts.configByUri ?? {},
            depsGraph: depGraph,
            graphKey: depPath,
            name: depNode.name,
            nodeVersion: opts.nodeVersion,
            patchFileHash: depNode.patch?.hash,
            pnprServer: opts.pnprServer,
            resolution: depNode.resolution,
            settings: opts.remoteSideEffectsCache,
            supportedArchitectures: opts.supportedArchitectures,
            upload,
            version: depNode.version,
          })
        }
      } catch (err: unknown) {
        assert(util.types.isNativeError(err))
        logger.warn({
          error: err,
          message: `An error occurred while uploading ${depNode.dir}`,
          prefix: opts.lockfileDir,
        })
      }
    }
    buildSucceeded = true
  } catch (err: unknown) {
    assert(util.types.isNativeError(err))
    if (depNode.optional) {
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
      return
    }
    throw err
  } finally {
    await slotLock?.release()
    if (buildSucceeded) {
      const hoistedLocationsOfDep = opts.hoistedLocations?.[depNode.depPath]
      if (hoistedLocationsOfDep) {
        // There is no need to build the same package in every location.
        // We just copy the built package to every location where it is present.
        const currentHoistedLocation = path.relative(opts.lockfileDir, depNode.dir)
        // The destinations must be resolved here, on the main thread: hardLinkDir()
        // runs on a worker thread, and applyPatchToDir() switches the process-wide
        // cwd, so a worker resolving a relative path inside that window would
        // resolve it against the wrong directory.
        const nonBuiltHoistedDeps = hoistedLocationsOfDep?.filter((hoistedLocation) => hoistedLocation !== currentHoistedLocation)
          .map((hoistedLocation) => path.join(opts.lockfileDir, hoistedLocation))
        await hardLinkDir(depNode.dir, nonBuiltHoistedDeps)
      }
    }
    if (opts.builtHoistedDeps) {
      opts.builtHoistedDeps[depNode.depPath].resolve()
    }
  }
}

/**
 * Serializes builds into one global virtual store slot across processes, and
 * marks the slot as mid-build before the build writes into it. Resolves to
 * `undefined` when another install built the slot while this one waited for
 * its lock, or when another install's build of it did not finish.
 */
async function lockSlotForBuild<T extends string> (depNode: DependenciesGraphNode<T>, lockfileDir: string): Promise<{ lock?: DirLock } | undefined> {
  const marker = path.join(depNode.dir, NEEDS_BUILD_MARKER)
  const awaitingBuild = await pathExists(marker)
  const lock = await lockGlobalVirtualStoreSlot(depNode.modules)
  if (await isStartedBuildMarker(marker) || (awaitingBuild && !await pathExists(marker))) {
    await lock?.release()
    return undefined
  }
  await markBuildStarted(depNode, lockfileDir)
  return { lock }
}

/**
 * Takes the lock that serializes writes into one global virtual store slot
 * across processes: builds, and re-imports of a slot that still carries its
 * `.pnpm-needs-build` marker. `slotModulesDir` is the slot's `node_modules`.
 * Resolves to `undefined` when the lock cannot be taken, and the caller then
 * writes without it: the lock avoids a race, and a race lost is better than
 * an install that refuses to run.
 */
export async function lockGlobalVirtualStoreSlot (slotModulesDir: string): Promise<DirLock | undefined> {
  const lockPath = path.join(path.dirname(slotModulesDir), SLOT_LOCK_DIR)
  try {
    return await DirLock.acquire(lockPath, { waitMs: SLOT_LOCK_WAIT_MS, abandonedMs: SLOT_LOCK_ABANDONED_MS })
  } catch (err: unknown) {
    logger.debug({ message: `Failed to lock ${lockPath}`, error: err })
    return undefined
  }
}

/**
 * Whether the slot's build started and then failed, or its process died,
 * leaving files the build may have changed. Only a re-import of the pristine
 * files, which rewrites the marker empty, makes it safe to build.
 */
async function isStartedBuildMarker (markerPath: string): Promise<boolean> {
  try {
    const content = await fs.readFile(markerPath, 'utf8')
    return content.trim() === STARTED_BUILD_MARKER_CONTENT
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return false
    throw err
  }
}

/**
 * A successful build removes the marker. A failed patch or build script, or a
 * process that dies mid-build, leaves it in place instead of the slot being
 * removed, because other projects may already link the shared slot. The next
 * install that reaches it then re-imports its pristine files and builds again.
 */
async function markBuildStarted<T extends string> (depNode: DependenciesGraphNode<T>, lockfileDir: string): Promise<void> {
  try {
    await fs.writeFile(path.join(depNode.dir, NEEDS_BUILD_MARKER), STARTED_BUILD_MARKER_CONTENT)
  } catch (err: unknown) {
    assert(util.types.isNativeError(err))
    if ('code' in err && err.code === 'ENOENT') return
    logger.warn({
      error: err,
      message: `Failed to mark ${depNode.dir} as mid-build`,
      prefix: lockfileDir,
    })
  }
}

export async function linkBinsOfDependencies<T extends string> (
  depNode: DependenciesGraphNode<T>,
  depGraph: DependenciesGraph<T>,
  opts: {
    extraNodePaths?: string[]
    optional: boolean
    preferSymlinkedExecutables?: boolean
    warn: (message: string) => void
  }
): Promise<void> {
  const childrenToLink: Record<string, T> = opts.optional
    ? depNode.children
    : pickBy((child, childAlias) => !depNode.optionalDependencies.has(childAlias), depNode.children)

  const binPath = path.join(depNode.dir, 'node_modules/.bin')

  const pkgNodes = [
    ...Object.entries(childrenToLink)
      .map(([alias, childDepPath]) => ({ alias, dep: depGraph[childDepPath] }))
      .filter(({ alias, dep }) => {
        if (!dep) {
          // TODO: Try to reproduce this issue with a test in @pnpm/installing.deps-installer
          logger.debug({ message: `Failed to link bins of "${alias}" to "${binPath}". This is probably not an issue.` })
          return false
        }
        return dep.hasBin && dep.installable !== false
      })
      .map(({ dep }) => dep),
    depNode,
  ]
  const pkgs = await Promise.all(pkgNodes
    .map(async (dep) => ({
      location: dep.dir,
      manifest: ((await dep.fetching?.())?.bundledManifest ?? (await safeReadPackageJsonFromDir(dep.dir))) as DependencyManifest ?? {},
    }))
  )

  await linkBinsOfPackages(pkgs, binPath, {
    extraNodePaths: opts.extraNodePaths,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
  })

  // link also the bundled dependencies` bins
  if (depNode.hasBundledDependencies) {
    const bundledModules = path.join(depNode.dir, 'node_modules')
    await linkBins(bundledModules, binPath, {
      extraNodePaths: opts.extraNodePaths,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      warn: opts.warn,
    })
  }
}

export async function linkBinsOfRuntimeDependencies<T extends string> (
  depNodes: Array<DependenciesGraphNode<T> | undefined>,
  binPath: string,
  opts: {
    extraNodePaths?: string[]
    preferSymlinkedExecutables?: boolean
  }
): Promise<void> {
  const runtimeNodes = depNodes.filter((dep): dep is DependenciesGraphNode<T> => dep != null && isRuntimeDepPath(dep.depPath))
  if (runtimeNodes.length === 0) return
  const pkgs = await Promise.all(runtimeNodes.map(async (dep) => ({
    location: dep.dir,
    manifest: ((await dep.fetching?.())?.bundledManifest ?? (await safeReadPackageJsonFromDir(dep.dir))) as DependencyManifest ?? {},
  })))
  await linkBinsOfPackages(pkgs, binPath, opts)
}
