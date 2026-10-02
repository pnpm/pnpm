import { getWorkspaceConcurrency } from '@pnpm/config.reader'
import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import type { DepsStateCache } from '@pnpm/deps.graph-hasher'
import { isError, PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import type { StoreController } from '@pnpm/store.controller-types'
import type {
  AllowBuild,
  DepPath,
  IgnoredBuilds,
  RegistryConfig,
  RemoteSideEffectsCacheSettings,
  SupportedArchitectures,
} from '@pnpm/types'
import { scheduleGraph, type TaskCompletion } from '@pnpm/workspace.task-scheduler'

import { buildDependency, type BuildDependencyOptions, buildIsAllowed } from './buildDependency.js'
import { buildGraph, type DependenciesGraph, type DependenciesGraphNode } from './buildGraph.js'

export { lockGlobalVirtualStoreSlot } from './globalVirtualStoreSlot.js'
export { linkBinsOfDependencies, linkBinsOfRuntimeDependencies } from './linkBinsOfDependencies.js'

export type { DepsStateCache }

export async function buildModules<NodeId extends string> (
  depGraph: DependenciesGraph<NodeId>,
  rootDepPaths: NodeId[],
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
    /**
     * The root project's `engines.runtime` Node version, which keys the
     * side-effects cache of every package that does not pin its own.
     */
    nodeVersion?: string
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
    engineStrict?: boolean
    /** Node version the installability check used. Separate from the script runner. */
    engineNodeVersion?: string
    /**
     * The `node_modules` directories of the installed projects and the hoisted
     * one. A skipped optional dependency's links are removed from them.
     */
    linkedModulesDirs?: string[]
    skipped?: Set<DepPath>
  }
): Promise<{ ignoredBuilds?: IgnoredBuilds }> {
  if (!rootDepPaths.length) return {}
  const warn = (message: string) => {
    logger.warn({ message, prefix: opts.lockfileDir })
  }

  const dependencyGraph = buildGraph<NodeId>(depGraph, rootDepPaths)
  if (dependencyGraph.size === 0) return {}
  const ignoredBuilds = new Set<DepPath>()
  const allowBuild = opts.allowBuild ?? (() => undefined)
  const shouldBuild = (depPath: NodeId): boolean => nodeNeedsBuild(depPath, depGraph, opts.depsToBuild)
  if (opts.frozenStore && opts.enableGlobalVirtualStore) {
    refuseBuildsIntoFrozenStore({ allowBuild, depGraph, depPaths: dependencyGraph.keys(), opts, shouldBuild })
  }
  await buildNodesInOrder(dependencyGraph, {
    buildDepOpts: {
      ...opts,
      allowBuild,
      builtHoistedDeps: opts.hoistedLocations ? {} : undefined,
      ignoredBuilds,
      warn,
    },
    childConcurrency: opts.childConcurrency,
    depGraph,
    shouldBuild,
  })
  return { ignoredBuilds }
}

function nodeNeedsBuild<NodeId extends string> (depPath: NodeId, depGraph: DependenciesGraph<NodeId>, depsToBuild: Set<string> | undefined): boolean {
  const node = depGraph[depPath]
  return (node.requiresBuild || node.patch != null) && !node.isBuilt &&
    (depsToBuild == null || depsToBuild.has(depPath))
}

interface BuildNodesContext<NodeId extends string> {
  buildDepOpts: BuildDependencyOptions
  childConcurrency?: number
  depGraph: DependenciesGraph<NodeId>
  shouldBuild: (depPath: NodeId) => boolean
}

async function buildNodesInOrder<NodeId extends string> (
  dependencyGraph: Map<NodeId, NodeId[]>,
  ctx: BuildNodesContext<NodeId>
): Promise<void> {
  const patchErrors: Error[] = []
  let firstError: unknown
  await scheduleGraph(dependencyGraph, {
    bail: true,
    concurrency: getWorkspaceConcurrency(ctx.childConcurrency),
    runNode: async (depPath): Promise<TaskCompletion> => {
      if (!ctx.shouldBuild(depPath)) return 'passed'
      try {
        await buildNode(depPath, ctx)
        return 'passed'
      } catch (err: unknown) {
        if (isPatchFailure(err)) {
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
}

async function buildNode<NodeId extends string> (depPath: NodeId, ctx: BuildNodesContext<NodeId>): Promise<void> {
  const { buildDepOpts, depGraph } = ctx
  const node = depGraph[depPath]
  const ignoreScripts = Boolean(buildDepOpts.ignoreScripts) ||
    (Boolean(node.requiresBuild) && !buildIsAllowed(node.depPath, buildDepOpts.allowBuild, buildDepOpts.ignoredBuilds))
  await buildDependency(depPath, depGraph, {
    ...buildDepOpts,
    ignoreScripts,
  })
}

function isPatchFailure (err: unknown): err is Error {
  return isError(err) && 'code' in err && err.code === 'ERR_PNPM_PATCH_FAILED'
}

/**
 * Under the global virtual store a package's directory lives inside the store
 * (`{storeDir}/links/...`), so applying a patch or running an allowlisted
 * lifecycle script writes into it. On a read-only `frozenStore` that write
 * would crash mid-build with a raw `EROFS`. A complete seed never reaches the
 * build step — built and patched packages are imported from the side-effects
 * cache with `isBuilt` set and filtered out by `shouldBuild` — so any package
 * still wanting to write means the seed is missing its build output. We
 * collect those off the same filtered graph and refuse up front instead of
 * failing cryptically once a script starts. Bin-linking reuses existing
 * symlinks write-free, and non-allowlisted scripts never run, so neither
 * counts as a blocking write. Optional dependencies don't block either — their
 * build failures are non-fatal at runtime, so their builds are skipped instead.
 */
function refuseBuildsIntoFrozenStore<NodeId extends string> (ctx: {
  allowBuild: AllowBuild
  depGraph: DependenciesGraph<NodeId>
  depPaths: Iterable<NodeId>
  opts: { ignoreScripts?: boolean, lockfileDir: string }
  shouldBuild: (depPath: NodeId) => boolean
}): void {
  const blocked = new Set<string>()
  for (const depPath of ctx.depPaths) {
    if (!ctx.shouldBuild(depPath)) continue
    const node = ctx.depGraph[depPath]
    if (!writesIntoPackageDir(node, ctx)) continue
    if (node.optional) {
      logSkippedFrozenOptional(node, ctx.opts.lockfileDir)
      continue
    }
    blocked.add(`${node.name}@${node.version}`)
  }
  if (blocked.size) {
    throwFrozenStoreNeedsBuild(blocked)
  }
}

function writesIntoPackageDir<NodeId extends string> (
  node: DependenciesGraphNode<NodeId>,
  ctx: { allowBuild: AllowBuild, opts: { ignoreScripts?: boolean } }
): boolean {
  // A patch is applied even under `ignoreScripts`, but a lifecycle script
  // is not — so only the patch write counts as blocking when scripts are
  // suppressed.
  const willPatch = node.patch != null
  const willRunScripts = !ctx.opts.ignoreScripts && Boolean(node.requiresBuild) && ctx.allowBuild(node.depPath) === true
  return willPatch || willRunScripts
}

/**
 * A build/patch failure on an optional dependency is non-fatal at runtime
 * (see the catch in `buildDependency`), so a seed missing an optional
 * package's build output skips that build instead of blocking the install.
 */
function logSkippedFrozenOptional<NodeId extends string> (node: DependenciesGraphNode<NodeId>, lockfileDir: string): void {
  skippedOptionalDependencyLogger.debug({
    details: `The read-only store (frozenStore) is missing the build output of ${node.name}@${node.version}.`,
    package: {
      id: node.dir,
      name: node.name,
      version: node.version,
    },
    prefix: lockfileDir,
    reason: 'build_failure',
  })
}

/** Refuse a build under a read-only global virtual store. See `refuseBuildsIntoFrozenStore`. */
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
