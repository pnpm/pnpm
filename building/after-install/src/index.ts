import assert from 'node:assert'
import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { linkBins } from '@pnpm/bins.linker'
import { pkgRequiresBuild } from '@pnpm/building.pkg-requires-build'
import { createAllowBuildFunction } from '@pnpm/building.policy'
import {
  LAYOUT_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import { calcDepState, type DepsStateCache, findRuntimeNodeVersion, iterateHashedGraphNodes, iteratePkgMeta, lockfileToDepGraph } from '@pnpm/deps.graph-hasher'
import { graphSequencer } from '@pnpm/deps.graph-sequencer'
import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import {
  runLifecycleHooksConcurrently,
  runPostinstallHooks,
} from '@pnpm/exec.lifecycle'
import { getContext, type PnpmContext } from '@pnpm/installing.context'
import { writeModulesManifest } from '@pnpm/installing.modules-yaml'
import type { TarballResolution } from '@pnpm/lockfile.types'
import {
  type LockfileObject,
  nameVerFromPkgSnapshot,
  packageIsIndependent,
  type PackageSnapshots,
} from '@pnpm/lockfile.utils'
import { lockfileWalker, type LockfileWalkerStep } from '@pnpm/lockfile.walker'
import { logger, streamParser } from '@pnpm/logger'
import npa from '@pnpm/npm-package-arg'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { createStoreController } from '@pnpm/store.connection-manager'
import { pickStoreIndexKey, ReadOnlyStoreIndex, StoreIndex } from '@pnpm/store.index'
import type {
  DepPath,
  IgnoredBuilds,
  PkgIdWithPatchHash,
  ProjectId,
  ProjectManifest,
  ProjectRootDir,
} from '@pnpm/types'
import { hardLinkDir } from '@pnpm/worker'
import pLimit from 'p-limit'
import { runGroups } from 'run-groups'
import semver from 'semver'

import {
  type BuildOptions,
  extendBuildOptions,
  type StrictBuildOptions,
} from './extendBuildOptions.js'

export type { BuildOptions }

// Serializes builds of a shared GVS projection across concurrent per-project
// rebuilds: the first build proceeds, concurrent ones await it and reuse the
// result. Keyed by the absolute projection directory.
const gvsBuildLocks = new Map<string, Promise<void>>()

function findPackages (
  packages: PackageSnapshots,
  searched: PackageSelector[],
  opts: {
    prefix: string
  }
): DepPath[] {
  return (Object.keys(packages) as DepPath[])
    .filter((relativeDepPath) => {
      const pkgLockfile = packages[relativeDepPath]
      const pkgInfo = nameVerFromPkgSnapshot(relativeDepPath, pkgLockfile)
      if (!pkgInfo.name) {
        logger.warn({
          message: `Skipping ${relativeDepPath} because cannot get the package name from ${WANTED_LOCKFILE}.
            Try to run \`pnpm update --depth 100\` to create a new ${WANTED_LOCKFILE} with all the necessary info.`,
          prefix: opts.prefix,
        })
        return false
      }
      return matches(searched, pkgInfo, dp.getPkgIdWithPatchHash(relativeDepPath))
    })
}

// TODO: move this logic to separate package as this is also used in tree-builder
function matches (
  searched: PackageSelector[],
  manifest: { name: string, version?: string },
  pkgIdWithPatchHash: PkgIdWithPatchHash
): boolean {
  return searched.some((searchedPkg) => {
    if (typeof searchedPkg === 'string') {
      return manifest.name === searchedPkg
    }
    if ('pkgIdWithPatchHash' in searchedPkg) {
      return searchedPkg.pkgIdWithPatchHash === pkgIdWithPatchHash
    }
    return searchedPkg.name === manifest.name && !!manifest.version &&
      semver.satisfies(manifest.version, searchedPkg.range)
  })
}

type PackageSelector = string | {
  name: string
  range: string
} | {
  /** A user-written depPath spec, normalized with the peer suffix stripped. */
  pkgIdWithPatchHash: string
}

export async function buildSelectedPkgs (
  projects: Array<{ buildIndex: number, manifest: ProjectManifest, rootDir: ProjectRootDir }>,
  pkgSpecs: string[],
  maybeOpts: BuildOptions
): Promise<{ ignoredBuilds?: IgnoredBuilds }> {
  const reporter = maybeOpts?.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  const opts = await extendBuildOptions(maybeOpts)
  const ctx = await getContext({ ...opts, allProjects: projects })

  if (ctx.currentLockfile?.packages == null) return {}
  const packages = ctx.currentLockfile.packages

  const searched: PackageSelector[] = pkgSpecs.map((arg) => {
    if (matchesDepPath(packages, arg)) {
      return { pkgIdWithPatchHash: dp.removePeersSuffix(arg) }
    }
    const { fetchSpec, name, raw, type } = npa(arg)
    if (raw === name) {
      return name
    }
    if (type !== 'version' && type !== 'range') {
      throw new Error(`Invalid argument - ${arg}. Rebuild can only select by version or range`)
    }
    return {
      name,
      range: fetchSpec,
    }
  })

  let pkgs = [] as string[]
  for (const { rootDir } of projects) {
    pkgs = [
      ...pkgs,
      ...findPackages(packages, searched, { prefix: rootDir }),
    ]
  }

  const { ignoredPkgs } = await _rebuild(
    {
      pkgsToRebuild: new Set(pkgs),
      ...ctx,
    },
    opts
  )
  await writeModulesManifest(ctx.rootModulesDir, {
    prunedAt: new Date().toUTCString(),
    ...ctx.modulesFile,
    hoistedDependencies: ctx.hoistedDependencies,
    hoistPattern: ctx.hoistPattern,
    included: ctx.include,
    ignoredBuilds: mergeIgnoredBuilds(ctx.modulesFile?.ignoredBuilds, ignoredPkgs, pkgs as DepPath[]),
    layoutVersion: LAYOUT_VERSION,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: ctx.pendingBuilds,
    publicHoistPattern: ctx.publicHoistPattern,
    registries: ctx.registries,
    skipped: Array.from(ctx.skipped),
    storeDir: ctx.modulesFile?.storeDir ?? ctx.storeDir,
    virtualStoreDir: ctx.modulesFile?.virtualStoreDir ?? ctx.virtualStoreDir,
    virtualStoreDirMaxLength: ctx.modulesFile?.virtualStoreDirMaxLength ?? ctx.virtualStoreDirMaxLength,
    allowBuilds: opts.allowBuilds,
  })
  return {
    ignoredBuilds: ignoredPkgs,
  }
}

function matchesDepPath (packages: PackageSnapshots, pkgSpec: string): boolean {
  const normalizedPkgSpec = dp.removePeersSuffix(pkgSpec)
  return Object.keys(packages).some((depPath) => dp.removePeersSuffix(depPath) === normalizedPkgSpec)
}

export async function buildProjects (
  projects: Array<{ buildIndex: number, manifest: ProjectManifest, rootDir: ProjectRootDir }>,
  maybeOpts: BuildOptions
): Promise<void> {
  const reporter = maybeOpts?.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  const opts = await extendBuildOptions(maybeOpts)
  const ctx = await getContext({ ...opts, allProjects: projects })

  let idsToRebuild: string[] = []

  if (opts.pending) {
    idsToRebuild = ctx.pendingBuilds
  } else if ((ctx.currentLockfile?.packages) != null) {
    idsToRebuild = Object.keys(ctx.currentLockfile.packages)
  }

  const { pkgsThatWereRebuilt, ignoredPkgs } = await _rebuild(
    {
      pkgsToRebuild: new Set(idsToRebuild),
      ...ctx,
    },
    opts
  )

  ctx.pendingBuilds = ctx.pendingBuilds.filter((depPath) => !pkgsThatWereRebuilt.has(depPath))

  const store = await createStoreController(opts)
  const scriptsOpts = {
    extraBinPaths: ctx.extraBinPaths,
    extraNodePaths: ctx.extraNodePaths,
    extraEnv: opts.extraEnv,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    scriptShell: opts.scriptShell,
    shellEmulator: opts.shellEmulator,
    storeController: store.ctrl,
    unsafePerm: opts.unsafePerm || false,
    userAgent: opts.userAgent,
  }
  await runLifecycleHooksConcurrently(
    ['preinstall', 'install', 'postinstall', 'prepublish', 'prepare'],
    Object.values(ctx.projects),
    opts.childConcurrency || 5,
    scriptsOpts
  )
  for (const { id, manifest } of Object.values(ctx.projects)) {
    if (((manifest?.scripts) != null) && (!opts.pending || ctx.pendingBuilds.includes(id))) {
      ctx.pendingBuilds.splice(ctx.pendingBuilds.indexOf(id), 1)
    }
  }

  await writeModulesManifest(ctx.rootModulesDir, {
    prunedAt: new Date().toUTCString(),
    ...ctx.modulesFile,
    hoistedDependencies: ctx.hoistedDependencies,
    hoistPattern: ctx.hoistPattern,
    included: ctx.include,
    ignoredBuilds: ignoredPkgs,
    layoutVersion: LAYOUT_VERSION,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: ctx.pendingBuilds,
    publicHoistPattern: ctx.publicHoistPattern,
    registries: ctx.registries,
    skipped: Array.from(ctx.skipped),
    storeDir: ctx.storeDir,
    virtualStoreDir: ctx.virtualStoreDir,
    virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
  })
}

function getSubgraphToBuild (
  step: LockfileWalkerStep,
  nodesToBuildAndTransitive: Set<DepPath>,
  opts: {
    pkgsToRebuild: Set<string>
  }
): boolean {
  let currentShouldBeBuilt = false
  for (const { depPath, next } of step.dependencies) {
    if (nodesToBuildAndTransitive.has(depPath)) {
      currentShouldBeBuilt = true
    }

    const childShouldBeBuilt = getSubgraphToBuild(next(), nodesToBuildAndTransitive, opts) ||
      opts.pkgsToRebuild.has(depPath)
    if (childShouldBeBuilt) {
      nodesToBuildAndTransitive.add(depPath)
      currentShouldBeBuilt = true
    }
  }
  for (const depPath of step.missing) {
    // It might make sense to fail if the depPath is not in the skipped list from .modules.yaml
    // However, the skipped list currently contains package IDs, not dep paths.
    logger.debug({ message: `No entry for "${depPath}" in ${WANTED_LOCKFILE}` })
  }
  return currentShouldBeBuilt
}

const limitLinking = pLimit(16)

async function _rebuild (
  ctx: {
    pkgsToRebuild: Set<string>
    skipped: Set<string>
    virtualStoreDir: string
    rootModulesDir: string
    currentLockfile: LockfileObject
    projects: Record<string, { id: ProjectId, rootDir: ProjectRootDir }>
    extraBinPaths: string[]
    extraNodePaths: string[]
  } & Pick<PnpmContext, 'modulesFile'>,
  opts: StrictBuildOptions
): Promise<{ pkgsThatWereRebuilt: Set<string>, ignoredPkgs: IgnoredBuilds }> {
  const depGraph = lockfileToDepGraph(ctx.currentLockfile, opts.supportedArchitectures)
  const depsStateCache: DepsStateCache = {}
  // Resolved `engines.runtime` Node version (when one is pinned) —
  // every side-effects-cache key computed below is anchored to it so
  // the prefix tracks the script-runner Node rather than pnpm's own
  // `process.version`.
  const nodeVersion = findRuntimeNodeVersion(Object.keys(depGraph))
  const pkgsThatWereRebuilt = new Set<string>()
  const graph = new Map()
  const pkgSnapshots: PackageSnapshots = ctx.currentLockfile.packages ?? {}

  const nodesToBuildAndTransitive = new Set<DepPath>()
  getSubgraphToBuild(
    lockfileWalker(
      ctx.currentLockfile,
      Object.values(ctx.projects).map(({ id }) => id),
      {
        include: {
          dependencies: opts.production,
          devDependencies: opts.development,
          optionalDependencies: opts.optional,
        },
      }
    ).step,
    nodesToBuildAndTransitive,
    { pkgsToRebuild: ctx.pkgsToRebuild }
  )
  const nodesToBuildAndTransitiveArray = Array.from(nodesToBuildAndTransitive)

  for (const depPath of nodesToBuildAndTransitiveArray) {
    const pkgSnapshot = pkgSnapshots[depPath]
    graph.set(depPath, Object.entries({ ...pkgSnapshot.dependencies, ...pkgSnapshot.optionalDependencies })
      .map(([pkgName, reference]) => dp.refToRelative(reference, pkgName))
      .filter((childRelDepPath) => childRelDepPath && nodesToBuildAndTransitive.has(childRelDepPath)))
  }
  const graphSequencerResult = graphSequencer(
    graph,
    nodesToBuildAndTransitiveArray
  )
  const chunks = graphSequencerResult.chunks as DepPath[][]
  const warn = (message: string) => {
    logger.info({ message, prefix: opts.dir })
  }

  const ignoredPkgs = new Set<DepPath>()
  const _allowBuild = createAllowBuildFunction(opts) ?? (() => undefined)
  const allowBuild = (depPath: DepPath) => {
    switch (_allowBuild(depPath)) {
      case true: return true
      case undefined: {
        ignoredPkgs.add(depPath)
        break
      }
    }
    return false
  }
  const builtDepPaths = new Set<string>()
  // This handle is read-only in practice (only `.get()` below); the
  // side-effects upload writes through `storeController`, not here. Open it
  // immutable under `frozenStore` so the read works against a read-only store
  // — a writable open would fail creating the WAL/`-shm` sidecar there. The
  // immutable open is gated on `frozenStore` because on a normal install the
  // concurrent side-effects uploads mutate `index.db`, which immutable reads
  // would not see.
  const storeIndex = opts.skipIfHasSideEffectsCache
    ? (opts.frozenStore ? new ReadOnlyStoreIndex(opts.storeDir) : new StoreIndex(opts.storeDir))
    : undefined

  // Under GVS, packages live at `<globalVirtualStoreDir>/<hash>/node_modules/<name>`,
  // not the classic virtualStoreDir layout. The hash is computed with the same inputs
  // as the installer so rebuild resolves the exact directory the install created.
  const gvsDirByDepPath = new Map<DepPath, string>()
  if (opts.enableGlobalVirtualStore) {
    const globalVirtualStoreDir = opts.globalVirtualStoreDir ?? path.join(opts.storeDir, 'links')
    for (const { hash, pkgMeta } of iterateHashedGraphNodes(
      depGraph,
      iteratePkgMeta(ctx.currentLockfile, depGraph),
      _allowBuild,
      opts.supportedArchitectures,
      nodeVersion
    )) {
      const preferredGvsDir = path.join(globalVirtualStoreDir, hash)
      gvsDirByDepPath.set(pkgMeta.depPath, fs.existsSync(preferredGvsDir)
        ? preferredGvsDir
        : findLinkedGvsDir(pkgMeta.name, Object.values(ctx.projects), globalVirtualStoreDir) ?? preferredGvsDir)
    }
  }
  const pkgModulesDir = (depPath: DepPath): string =>
    gvsDirByDepPath.has(depPath)
      ? path.join(gvsDirByDepPath.get(depPath)!, 'node_modules')
      : path.join(ctx.virtualStoreDir, dp.depPathToFilename(depPath, opts.virtualStoreDirMaxLength), 'node_modules')

  const groups = chunks.map((chunk) => chunk.filter((depPath) => ctx.pkgsToRebuild.has(depPath) && !ctx.skipped.has(depPath)).map((depPath) =>
    async () => {
      const pkgSnapshot = pkgSnapshots[depPath]
      const pkgInfo = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
      const pkgRoots = opts.nodeLinker === 'hoisted'
        ? (ctx.modulesFile?.hoistedLocations?.[depPath] ?? []).map((hoistedLocation) => path.join(opts.lockfileDir, hoistedLocation))
        : [path.join(pkgModulesDir(depPath), pkgInfo.name)]
      if (pkgRoots.length === 0) {
        if (pkgSnapshot.optional) return
        throw new PnpmError('MISSING_HOISTED_LOCATIONS', `${depPath} is not found in hoistedLocations inside node_modules/.modules.yaml`, {
          hint: 'If you installed your node_modules with pnpm older than v7.19.0, you may need to remove it and run "pnpm install"',
        })
      }
      const pkgRoot = pkgRoots[0]
      // If another project is already building this shared projection, wait for it
      // and reuse the result instead of racing on the same directory.
      const gvsDir = gvsDirByDepPath.get(depPath)
      if (gvsDir != null) {
        const inFlight = gvsBuildLocks.get(gvsDir)
        if (inFlight != null) {
          await inFlight.catch(() => {})
          pkgsThatWereRebuilt.add(depPath)
          return
        }
      }
      let releaseGvsLock: (() => void) | undefined
      if (gvsDir != null) {
        let resolveLock!: () => void
        gvsBuildLocks.set(gvsDir, new Promise<void>((resolve) => {
          resolveLock = resolve
        }))
        releaseGvsLock = () => {
          gvsBuildLocks.delete(gvsDir)
          resolveLock()
        }
      }
      try {
        const extraBinPaths = ctx.extraBinPaths
        if (opts.nodeLinker !== 'hoisted') {
          const modules = pkgModulesDir(depPath)
          const binPath = path.join(pkgRoot, 'node_modules', '.bin')
          await linkBins(modules, binPath, { extraNodePaths: ctx.extraNodePaths, warn })
        } else {
          extraBinPaths.push(...binDirsInAllParentDirs(pkgRoot, opts.lockfileDir))
        }
        const resolution = (pkgSnapshot.resolution as TarballResolution)
        let sideEffectsCacheKey: string | undefined
        // Match the resolver-supplied pkg.id used by the writer in
        // @pnpm/installing.package-requester: that's the tarball URL for
        // git-hosted packages (nonSemverVersion) and `name@version` otherwise.
        const pkgId = pkgInfo.nonSemverVersion ?? `${pkgInfo.name}@${pkgInfo.version}`
        if (opts.skipIfHasSideEffectsCache && (resolution.gitHosted || resolution.integrity)) {
          const filesIndexFile = pickStoreIndexKey(resolution, pkgId, { built: true })
          const pkgFilesIndex = storeIndex!.get(filesIndexFile) as PackageFilesIndex | undefined
          if (pkgFilesIndex) {
            sideEffectsCacheKey = calcDepState(depGraph, depsStateCache, depPath, {
              includeDepGraphHash: true,
              supportedArchitectures: opts.supportedArchitectures,
              nodeVersion,
            })
            if (pkgFilesIndex.sideEffects?.has(sideEffectsCacheKey)) {
              pkgsThatWereRebuilt.add(depPath)
              return
            }
          }
        }
        let requiresBuild = true
        const pgkManifest = await safeReadPackageJsonFromDir(pkgRoot)
        if (pgkManifest != null) {
          // This won't return the correct result for packages with binding.gyp as we don't pass the filesIndex to the function.
          // However, currently rebuild doesn't work for such packages at all, which should be fixed.
          requiresBuild = pkgRequiresBuild(pgkManifest, new Map())
        }

        const hasSideEffects = requiresBuild && allowBuild(depPath) && await runPostinstallHooks({
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
        if (hasSideEffects && (opts.sideEffectsCacheWrite ?? true) && (resolution.gitHosted || resolution.integrity)) {
          builtDepPaths.add(depPath)
          const filesIndexFile = pickStoreIndexKey(resolution, pkgId, { built: true })
          try {
            if (!sideEffectsCacheKey) {
              sideEffectsCacheKey = calcDepState(depGraph, depsStateCache, depPath, {
                includeDepGraphHash: true,
                nodeVersion,
              })
            }
            await opts.storeController.upload(pkgRoot, {
              sideEffectsCacheKey,
              filesIndexFile,
            })
          } catch (err: unknown) {
            assert(util.types.isNativeError(err))
            logger.warn({
              error: err,
              message: `An error occurred while uploading ${pkgRoot}`,
              prefix: opts.lockfileDir,
            })
          }
        }
        pkgsThatWereRebuilt.add(depPath)
      } catch (err: unknown) {
        assert(util.types.isNativeError(err))
        if (pkgSnapshot.optional) {
          // TODO: add parents field to the log
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
          return
        }
        throw err
      } finally {
        releaseGvsLock?.()
      }
      if (pkgRoots.length > 1) {
        await hardLinkDir(pkgRoot, pkgRoots.slice(1))
      }
    }
  ))

  await runGroups(opts.childConcurrency || 5, groups)
  storeIndex?.close()

  if (builtDepPaths.size > 0) {
    // It may be optimized because some bins were already linked before running lifecycle scripts
    await Promise.all(
      (Object
        .keys(pkgSnapshots) as DepPath[])
        .filter((depPath) => !packageIsIndependent(pkgSnapshots[depPath]))
        .map(async (depPath) => limitLinking(async () => {
          const pkgSnapshot = pkgSnapshots[depPath]
          const pkgInfo = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
          const modules = pkgModulesDir(depPath)
          const binPath = path.join(modules, pkgInfo.name, 'node_modules', '.bin')
          return linkBins(modules, binPath, { warn })
        }))
    )
    await Promise.all(Object.values(ctx.projects).map(async ({ rootDir }) => limitLinking(async () => {
      const modules = path.join(rootDir, 'node_modules')
      const binPath = path.join(modules, '.bin')
      return linkBins(modules, binPath, {
        allowExoticManifests: true,
        warn,
      })
    })))
  }

  return { pkgsThatWereRebuilt, ignoredPkgs }
}

// TODO: delete once rebuild relocates GVS projections to the newly computed
// hash instead of building in place (https://github.com/pnpm/pnpm/issues/12302).
function findLinkedGvsDir (
  pkgName: string,
  projects: Array<{ rootDir: ProjectRootDir }>,
  globalVirtualStoreDir: string
): string | undefined {
  const normalizedGvsRoot = `${path.resolve(globalVirtualStoreDir)}${path.sep}`
  for (const { rootDir } of projects) {
    const pkgLink = path.join(rootDir, 'node_modules', pkgName)
    try {
      const target = fs.readlinkSync(pkgLink)
      const pkgRoot = path.resolve(path.dirname(pkgLink), target)
      if (!pkgRoot.startsWith(normalizedGvsRoot)) continue
      return nthAncestorDir(pkgRoot, pkgName.split('/').length + 1)
    } catch (err: unknown) {
      // EINVAL: pkgLink exists but is not a symlink.
      if (util.types.isNativeError(err) && 'code' in err && (err.code === 'EINVAL' || err.code === 'ENOENT')) continue
      throw err
    }
  }
  return undefined
}

function nthAncestorDir (dir: string, levels: number): string {
  let result = dir
  for (let i = 0; i < levels; i++) {
    result = path.dirname(result)
  }
  return result
}

function binDirsInAllParentDirs (pkgRoot: string, lockfileDir: string): string[] {
  const binDirs: string[] = []
  let dir = pkgRoot
  do {
    if (!(path.dirname(dir)[0] === '@')) {
      binDirs.push(path.join(dir, 'node_modules/.bin'))
    }
    dir = path.dirname(dir)
  } while (path.relative(dir, lockfileDir) !== '')
  binDirs.push(path.join(lockfileDir, 'node_modules/.bin'))
  return binDirs
}

/**
 * Merge new ignoredBuilds from a selective rebuild with existing ones.
 * Keeps existing entries for packages that weren't part of this rebuild.
 */
function mergeIgnoredBuilds (
  existing: IgnoredBuilds | undefined,
  newIgnored: IgnoredBuilds,
  rebuiltPkgs: DepPath[]
): IgnoredBuilds | undefined {
  if (!existing?.size && !newIgnored.size) return undefined
  const rebuiltSet = new Set<DepPath>(rebuiltPkgs)
  const merged = new Set<DepPath>()
  if (existing) {
    for (const depPath of existing) {
      if (!rebuiltSet.has(depPath)) {
        merged.add(depPath)
      }
    }
  }
  for (const depPath of newIgnored) {
    merged.add(depPath)
  }
  return merged.size ? merged : undefined
}
