import assert from 'node:assert'
import path from 'node:path'

import { relinkModulesDirPackageBins } from '@pnpm/bins.linker'
import { dirRequiresBuild } from '@pnpm/building.pkg-requires-build'
import { packageIsInstallable } from '@pnpm/config.package-is-installable'
import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import type { DepsStateCache } from '@pnpm/deps.graph-hasher'
import { isError, PnpmError } from '@pnpm/error'
import { runPostinstallHooks } from '@pnpm/exec.lifecycle'
import { applyPatchToDir } from '@pnpm/patching.apply-patch'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { StoreController } from '@pnpm/store.controller-types'
import type {
  AllowBuild,
  DepPath,
  HoistedDependencies,
  RegistryConfig,
  RemoteSideEffectsCacheSettings,
  SupportedArchitectures,
} from '@pnpm/types'
import { hardLinkDir } from '@pnpm/worker'
import { isSubdir } from 'is-subdir'
import pDefer, { type DeferredPromise } from 'p-defer'

import type { DependenciesGraph, DependenciesGraphNode } from './buildGraph.js'
import { discardStagingDir, isFromLocalDirectory, isPackagePlaced, placeBuiltPackage, replacePlacedPackage } from './globalVirtualStoreSlot.js'
import { linkBinsOfDependencies } from './linkBinsOfDependencies.js'
import {
  removeIncompatibleOptional,
  removeLinksToSkippedOptionalDependency,
  removeSkippedOptionalDependency,
} from './removeSkippedDependency.js'
import { type BuildOutput, storeBuildOutput } from './storeBuildOutput.js'

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
  hoistedDependencies?: HoistedDependencies
  hoistedModulesDir?: string
  builtDepPaths?: Set<DepPath>
  builtPackageDirs?: Set<string>
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
    state.succeeded = depNode.stagingDir == null
      ? await buildInPackageDir(depPath, depGraph, opts)
      : await buildInStagingDir(depPath, depGraph, opts)
  } catch (err: unknown) {
    await skipFailedOptionalBuild(err, depNode, opts)
  } finally {
    try {
      if (state.succeeded) {
        await copyToOtherHoistedLocations(depNode, opts)
        await relinkBuiltHoistedBins(depNode, opts)
      }
    } finally {
      opts.builtHoistedDeps?.[depNode.depPath].resolve()
    }
  }
}

async function buildInPackageDir<NodeId extends string> (
  depPath: NodeId,
  depGraph: DependenciesGraph<NodeId>,
  opts: BuildDependencyOptions
): Promise<boolean> {
  const built = await buildPackage(depPath, depGraph, depGraph[depPath], opts)
  if (built == null) return false
  if (built.output != null) await storeBuildOutput(built.output)
  return true
}

/**
 * Builds a package that the link phase staged for its global virtual store
 * slot, then renames it into place. A package another install placed first is
 * not built, unless it comes from a local directory, whose build replaces the
 * placed package. A failed build leaves nothing in the slot. The build output
 * is cached only once the package is placed.
 */
async function buildInStagingDir<NodeId extends string> (
  depPath: NodeId,
  depGraph: DependenciesGraph<NodeId>,
  opts: BuildDependencyOptions
): Promise<boolean> {
  const depNode = depGraph[depPath]
  const stagingDir = depNode.stagingDir!
  const replacesPlaced = isFromLocalDirectory(depNode)
  if (!replacesPlaced && await isPackagePlaced(depNode.dir)) {
    await discardStagingDir(stagingDir)
    // The placed package's bins may differ from those linked before the build.
    if (depNode.patch != null || depNode.hasBin) {
      opts.builtDepPaths?.add(depNode.depPath)
      opts.builtPackageDirs?.add(path.normalize(depNode.dir))
    }
    return true
  }
  let built: BuiltPackage<NodeId> | undefined
  let staged = true
  try {
    built = await buildPackage(depPath, depGraph, { ...depNode, dir: stagingDir }, opts)
    if (built == null) return false
    if (replacesPlaced) {
      await replacePlacedPackage(stagingDir, depNode.dir)
      staged = false
    } else {
      staged = await placeBuiltPackage(stagingDir, depNode.dir) === 'placedByOther'
    }
  } finally {
    if (staged) await discardStagingDir(stagingDir)
  }
  if (built.output != null) await storeBuildOutput({ ...built.output, pkgNode: depNode })
  return true
}

interface BuiltPackage<NodeId extends string> {
  /** The build output to cache. Absent when the build was withheld. */
  output?: BuildOutput<NodeId>
}

/**
 * Patches and builds `pkgNode`, which is `depGraph[depPath]` or a copy of it
 * located in a staging directory. Resolves to `undefined` when a patch made an
 * optional package uninstallable.
 */
async function buildPackage<NodeId extends string> (
  depPath: NodeId,
  depGraph: DependenciesGraph<NodeId>,
  pkgNode: DependenciesGraphNode<NodeId>,
  opts: BuildDependencyOptions
): Promise<BuiltPackage<NodeId> | undefined> {
  await linkBinsOfDependencies(pkgNode, depGraph, opts)
  const patchOutcome = await patchDependency(depPath, depGraph, pkgNode, opts)
  if (patchOutcome === 'uninstallable') return undefined
  const isPatched = patchOutcome === 'patched'
  const { requiresBuild, ignoreScripts } = await gateBuildScripts(pkgNode, isPatched, opts)
  const hasSideEffects = !ignoreScripts && await runBuildScripts(depPath, pkgNode, opts)
  if (isPatched || (hasSideEffects && pkgNode.hasBin)) {
    opts.builtDepPaths?.add(pkgNode.depPath)
    opts.builtPackageDirs?.add(path.normalize(depGraph[depPath].dir))
    await linkBinsOfDependencies(pkgNode, depGraph, {
      ...opts,
      builtPackageDirs: new Set([...opts.builtPackageDirs ?? [], path.normalize(pkgNode.dir)]),
    })
  }
  // A package whose build was withheld - the allow-build policy said so, or
  // ignoreScripts did - must not be cached as if it were built. The entry
  // the patch alone produced would replay on the install that finally runs
  // the build, and the scripts would never get their chance.
  const buildPending = requiresBuild && ignoreScripts
  return { output: buildPending ? undefined : { depPath, depGraph, pkgNode, opts, isPatched, hasSideEffects } }
}

async function patchDependency<NodeId extends string> (
  depPath: NodeId,
  depGraph: DependenciesGraph<NodeId>,
  depNode: DependenciesGraphNode<NodeId>,
  opts: BuildDependencyOptions
): Promise<PatchOutcome> {
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
  await removeIncompatibleOptional(depPath, depGraph[depPath], depGraph, opts)
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

async function skipFailedOptionalBuild<NodeId extends string> (
  err: unknown,
  depNode: DependenciesGraphNode<NodeId>,
  opts: BuildDependencyOptions
): Promise<void> {
  assert(isError(err))
  if (!depNode.optional) throw err
  // A global virtual store slot is shared, so only this install's links to it are removed.
  if (depNode.stagingDir != null) {
    await removeLinksToSkippedOptionalDependency(depNode, opts)
  } else if (!opts.enableGlobalVirtualStore) {
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

export async function relinkBuiltHoistedBins<NodeId extends string> (
  depNode: Pick<DependenciesGraphNode<NodeId>, 'depPath'>,
  opts: Pick<BuildDependencyOptions, 'builtDepPaths' | 'extraNodePaths' | 'hoistedDependencies' | 'hoistedLocations' | 'hoistedModulesDir' | 'lockfileDir' | 'preferSymlinkedExecutables' | 'warn'>
): Promise<void> {
  if (!opts.builtDepPaths?.has(depNode.depPath)) return
  const hoistedRoots = getBuiltHoistedPkgRoots(depNode.depPath, opts)
  if (hoistedRoots.length === 0) return
  await relinkModulesDirPackageBins(hoistedRoots, opts.builtDepPaths, {
    extraNodePaths: opts.extraNodePaths,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
    warn: opts.warn,
  })
}

function getBuiltHoistedPkgRoots (
  depPath: DepPath,
  opts: Pick<BuildDependencyOptions, 'hoistedDependencies' | 'hoistedLocations' | 'hoistedModulesDir' | 'lockfileDir'>
): string[] {
  if (opts.hoistedLocations != null) {
    return (opts.hoistedLocations[depPath] ?? []).map((hoistedLocation) => path.join(opts.lockfileDir, hoistedLocation))
  }
  if (opts.hoistedModulesDir == null) return []
  const aliases = opts.hoistedDependencies?.[depPath] ?? {}
  return Object.entries(aliases)
    .filter(([, visibility]) => visibility === 'private')
    .map(([alias]) => safeJoinHoistedAlias(opts.hoistedModulesDir!, alias))
}

const RESERVED_HOIST_ALIASES = new Set(['.bin', '.pnpm', 'node_modules', 'favicon.ico'])

function safeJoinHoistedAlias (modulesDir: string, alias: string): string {
  const parts = alias.split('/')
  const valid = (parts.length === 1 && isValidSegment(parts[0])) ||
    (parts.length === 2 && parts[0].startsWith('@') && isValidSegment(parts[0].slice(1)) && isValidSegment(parts[1]))
  const link = path.join(modulesDir, ...parts)
  if (!valid || RESERVED_HOIST_ALIASES.has(alias.toLowerCase()) || !isSubdir(modulesDir, link)) {
    throw new PnpmError('INVALID_DEPENDENCY_NAME', `Dependency name "${alias}" cannot be linked into ${modulesDir}`)
  }
  return link
}

function isValidSegment (segment: string): boolean {
  return segment.length > 0 && !/^[._-]/.test(segment) && encodeURIComponent(segment) === segment
}
