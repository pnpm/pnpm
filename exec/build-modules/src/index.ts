import assert from 'node:assert'
import path from 'node:path'
import util from 'node:util'
import { calcDepState, type DepsStateCache } from '@pnpm/calc-dep-state'
import { getWorkspaceConcurrency } from '@pnpm/config'
import { skippedOptionalDependencyLogger, ignoredScriptsLogger } from '@pnpm/core-loggers'
import { runPostinstallHooks } from '@pnpm/lifecycle'
import { linkBins, linkBinsOfPackages } from '@pnpm/link-bins'
import { logger } from '@pnpm/logger'
import { hardLinkDir } from '@pnpm/worker'
import { readPackageJsonFromDir, safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { type StoreController } from '@pnpm/store-controller-types'
import { applyPatchToDir } from '@pnpm/patching.apply-patch'
import { type DependencyManifest } from '@pnpm/types'
import pDefer, { type DeferredPromise } from 'p-defer'
import pickBy from 'ramda/src/pickBy'
import runGroups from 'run-groups'
import { buildSequence, type DependenciesGraph, type DependenciesGraphNode } from './buildSequence'

export type { DepsStateCache }

export async function buildModules<T extends string> (
  depGraph: DependenciesGraph<T>,
  rootDepPaths: T[],
  opts: {
    allowBuild?: (pkgName: string) => boolean
    ignorePatchFailures?: boolean
    ignoredBuiltDependencies?: string[]
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
    rawConfig: object
    unsafePerm: boolean
    userAgent: string
    scriptsPrependNodePath?: boolean | 'warn-only'
    scriptShell?: string
    shellEmulator?: boolean
    sideEffectsCacheWrite: boolean
    storeController: StoreController
    rootModulesDir: string
    hoistedLocations?: Record<string, string[]>
  }
): Promise<{ ignoredBuilds?: string[] }> {
  if (!rootDepPaths.length) return {}
  const warn = (message: string) => {
    logger.warn({ message, prefix: opts.lockfileDir })
  }
  // postinstall hooks

  const buildDepOpts = {
    ...opts,
    builtHoistedDeps: opts.hoistedLocations ? {} : undefined,
    warn,
  }
  const chunks = buildSequence<T>(depGraph, rootDepPaths)
  if (!chunks.length) return {}
  const ignoredPkgs = new Set<string>()
  const allowBuild = opts.allowBuild ?? (() => true)
  const groups = chunks.map((chunk) => {
    chunk = chunk.filter((depPath) => {
      const node = depGraph[depPath]
      return (node.requiresBuild || node.patch != null) && !node.isBuilt
    })
    if (opts.depsToBuild != null) {
      chunk = chunk.filter((depPath) => opts.depsToBuild!.has(depPath))
    }

    return chunk.map((depPath) =>
      () => {
        let ignoreScripts = Boolean(buildDepOpts.ignoreScripts)
        if (!ignoreScripts) {
          if (depGraph[depPath].requiresBuild && !allowBuild(depGraph[depPath].name)) {
            ignoredPkgs.add(depGraph[depPath].name)
            ignoreScripts = true
          }
        }
        return buildDependency(depPath, depGraph, {
          ...buildDepOpts,
          ignoreScripts,
        })
      }
    )
  })
  await runGroups(getWorkspaceConcurrency(opts.childConcurrency), groups)
  if (opts.ignoredBuiltDependencies?.length) {
    for (const ignoredBuild of opts.ignoredBuiltDependencies) {
      // We already ignore the build of this dependency.
      // No need to report it.
      ignoredPkgs.delete(ignoredBuild)
    }
  }
  const packageNames = Array.from(ignoredPkgs)
  ignoredScriptsLogger.debug({ packageNames })
  return { ignoredBuilds: packageNames }
}

async function buildDependency<T extends string> (
  depPath: T,
  depGraph: DependenciesGraph<T>,
  opts: {
    ignorePatchFailures?: boolean
    extraBinPaths?: string[]
    extraNodePaths?: string[]
    extraEnv?: Record<string, string>
    depsStateCache: DepsStateCache
    ignoreScripts?: boolean
    lockfileDir: string
    optional: boolean
    preferSymlinkedExecutables?: boolean
    rawConfig: object
    rootModulesDir: string
    scriptsPrependNodePath?: boolean | 'warn-only'
    scriptShell?: string
    shellEmulator?: boolean
    sideEffectsCacheWrite: boolean
    storeController: StoreController
    unsafePerm: boolean
    hoistedLocations?: Record<string, string[]>
    builtHoistedDeps?: Record<string, DeferredPromise<void>>
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
  try {
    await linkBinsOfDependencies(depNode, depGraph, opts)
    let isPatched = false
    if (depNode.patch) {
      const { file, strict } = depNode.patch
      // `strict` is a legacy property which was kept to preserve backward compatibility.
      // Once a major version of pnpm is released, `strict` should be removed completely.
      const allowFailure: boolean = opts.ignorePatchFailures ?? !strict
      isPatched = applyPatchToDir({ allowFailure, patchedDir: depNode.dir, patchFilePath: file.path })
    }
    const hasSideEffects = !opts.ignoreScripts && await runPostinstallHooks({
      depPath,
      extraBinPaths: opts.extraBinPaths,
      extraEnv: opts.extraEnv,
      initCwd: opts.lockfileDir,
      optional: depNode.optional,
      pkgRoot: depNode.dir,
      rawConfig: opts.rawConfig,
      rootModulesDir: opts.rootModulesDir,
      scriptsPrependNodePath: opts.scriptsPrependNodePath,
      scriptShell: opts.scriptShell,
      shellEmulator: opts.shellEmulator,
      unsafePerm: opts.unsafePerm || false,
    })
    if ((isPatched || hasSideEffects) && opts.sideEffectsCacheWrite) {
      try {
        const sideEffectsCacheKey = calcDepState(depGraph, opts.depsStateCache, depPath, {
          patchFileHash: depNode.patch?.file.hash,
          includeDepGraphHash: hasSideEffects,
        })
        await opts.storeController.upload(depNode.dir, {
          sideEffectsCacheKey,
          filesIndexFile: depNode.filesIndexFile,
        })
      } catch (err: unknown) {
        assert(util.types.isNativeError(err))
        if (err && 'statusCode' in err && err.statusCode === 403) {
          logger.warn({
            message: `The store server disabled upload requests, could not upload ${depNode.dir}`,
            prefix: opts.lockfileDir,
          })
        } else {
          logger.warn({
            error: err,
            message: `An error occurred while uploading ${depNode.dir}`,
            prefix: opts.lockfileDir,
          })
        }
      }
    }
  } catch (err: unknown) {
    assert(util.types.isNativeError(err))
    if (depNode.optional) {
      // TODO: add parents field to the log
      const pkg = await readPackageJsonFromDir(path.join(depNode.dir)) as DependencyManifest
      skippedOptionalDependencyLogger.debug({
        details: err.toString(),
        package: {
          id: depNode.dir,
          name: pkg.name,
          version: pkg.version,
        },
        prefix: opts.lockfileDir,
        reason: 'build_failure',
      })
      return
    }
    throw err
  } finally {
    const hoistedLocationsOfDep = opts.hoistedLocations?.[depNode.depPath]
    if (hoistedLocationsOfDep) {
      // There is no need to build the same package in every location.
      // We just copy the built package to every location where it is present.
      const currentHoistedLocation = path.relative(opts.lockfileDir, depNode.dir)
      const nonBuiltHoistedDeps = hoistedLocationsOfDep?.filter((hoistedLocation) => hoistedLocation !== currentHoistedLocation)
      await hardLinkDir(depNode.dir, nonBuiltHoistedDeps)
    }
    if (opts.builtHoistedDeps) {
      opts.builtHoistedDeps[depNode.depPath].resolve()
    }
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
          // TODO: Try to reproduce this issue with a test in @pnpm/core
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
      manifest: await dep.fetchingBundledManifest?.() ?? (await safeReadPackageJsonFromDir(dep.dir) as DependencyManifest) ?? {},
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
