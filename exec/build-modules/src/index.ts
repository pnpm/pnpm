import path from 'path'
import { calcDepState, type DepsStateCache } from '@pnpm/calc-dep-state'
import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import { runPostinstallHooks } from '@pnpm/lifecycle'
import { linkBins, linkBinsOfPackages } from '@pnpm/link-bins'
import { logger } from '@pnpm/logger'
import { hardLinkDir } from '@pnpm/fs.hard-link-dir'
import { readPackageJsonFromDir, safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { type StoreController } from '@pnpm/store-controller-types'
import { applyPatchToDir } from '@pnpm/patching.apply-patch'
import { type DependencyManifest } from '@pnpm/types'
import pDefer, { type DeferredPromise } from 'p-defer'
import pickBy from 'ramda/src/pickBy'
import runGroups from 'run-groups'
import { buildSequence, type DependenciesGraph, type DependenciesGraphNode } from './buildSequence'

export type { DepsStateCache }

export async function buildModules (
  depGraph: DependenciesGraph,
  rootDepPaths: string[],
  opts: {
    allowBuild?: (pkgName: string) => boolean
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
) {
  const warn = (message: string) => {
    logger.warn({ message, prefix: opts.lockfileDir })
  }
  // postinstall hooks

  const buildDepOpts = {
    ...opts,
    builtHoistedDeps: opts.hoistedLocations ? {} : undefined,
    warn,
  }
  const chunks = buildSequence(depGraph, rootDepPaths)
  const allowBuild = opts.allowBuild ?? (() => true)
  const groups = chunks.map((chunk) => {
    chunk = chunk.filter((depPath) => {
      const node = depGraph[depPath]
      return (node.requiresBuild || node.patchFile != null) && !node.isBuilt
    })
    if (opts.depsToBuild != null) {
      chunk = chunk.filter((depPath) => opts.depsToBuild!.has(depPath))
    }

    return chunk.map((depPath: string) =>
      async () => {
        return buildDependency(depPath, depGraph, {
          ...buildDepOpts,
          ignoreScripts: Boolean(buildDepOpts.ignoreScripts) || !allowBuild(depGraph[depPath].name),
        })
      }
    )
  })
  await runGroups(opts.childConcurrency ?? 4, groups)
}

async function buildDependency (
  depPath: string,
  depGraph: DependenciesGraph,
  opts: {
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
) {
  const depNode = depGraph[depPath]
  if (opts.builtHoistedDeps) {
    if (opts.builtHoistedDeps[depNode.depPath]) {
      await opts.builtHoistedDeps[depNode.depPath].promise
      return
    }
    opts.builtHoistedDeps[depNode.depPath] = pDefer()
  }
  try {
    await linkBinsOfDependencies(depNode, depGraph, opts)
    const isPatched = depNode.patchFile?.path != null
    if (isPatched) {
      applyPatchToDir({ patchedDir: depNode.dir, patchFilePath: depNode.patchFile!.path })
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
          patchFileHash: depNode.patchFile?.hash,
          isBuilt: hasSideEffects,
        })
        await opts.storeController.upload(depNode.dir, {
          sideEffectsCacheKey,
          filesIndexFile: depNode.filesIndexFile,
        })
      } catch (err: any) { // eslint-disable-line
        if (err.statusCode === 403) {
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
  } catch (err: any) { // eslint-disable-line
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

export async function linkBinsOfDependencies (
  depNode: DependenciesGraphNode,
  depGraph: DependenciesGraph,
  opts: {
    extraNodePaths?: string[]
    optional: boolean
    preferSymlinkedExecutables?: boolean
    warn: (message: string) => void
  }
) {
  const childrenToLink: Record<string, string> = opts.optional
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
