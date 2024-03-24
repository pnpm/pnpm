import '@total-typescript/ts-reset'

import path from 'node:path'

import runGroups from 'run-groups'
import pickBy from 'ramda/src/pickBy'
import pDefer, { type DeferredPromise } from 'p-defer'

import { logger } from '@pnpm/logger'
import { hardLinkDir } from '@pnpm/worker'
import { calcDepState } from '@pnpm/calc-dep-state'
import { runPostinstallHooks } from '@pnpm/lifecycle'
import { applyPatchToDir } from '@pnpm/patching.apply-patch'
import { linkBins, linkBinsOfPackages } from '@pnpm/link-bins'
import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import { readPackageJsonFromDir, safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import type { DependencyManifest, StoreController, GenericDependenciesGraph, DependenciesGraphNode, PackageManifest, DepsStateCache } from '@pnpm/types'

import { buildSequence } from './buildSequence.js'

export async function buildModules(
  depGraph: GenericDependenciesGraph<DependenciesGraphNode>,
  rootDepPaths: string[],
  opts: {
    allowBuild?: ((pkgName: string) => boolean) | undefined
    childConcurrency?: number | undefined
    depsToBuild?: Set<string> | undefined
    depsStateCache: DepsStateCache
    extraBinPaths?: string[] | undefined
    extraNodePaths?: string[] | undefined
    extraEnv?: Record<string, string> | undefined
    ignoreScripts?: boolean | undefined
    lockfileDir: string
    optional?: boolean | undefined
    preferSymlinkedExecutables?: boolean | undefined
    rawConfig: object
    unsafePerm: boolean
    userAgent: string
    scriptsPrependNodePath?: boolean | 'warn-only' | undefined
    scriptShell?: string | undefined
    shellEmulator?: boolean | undefined
    sideEffectsCacheWrite: boolean
    storeController: StoreController
    rootModulesDir: string
    hoistedLocations?: Record<string, string[]> | undefined
  }
) {
  function warn(message: string): void {
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

      return (node?.requiresBuild || node?.patchFile != null) && !node.isBuilt
    })

    if (opts.depsToBuild != null) {
      chunk = chunk.filter((depPath): boolean => {
        return opts.depsToBuild?.has(depPath) ?? false;
      })
    }

    return chunk.map((depPath: string) => async () => {
      return buildDependency(depPath, depGraph, {
        ...buildDepOpts,
        ignoreScripts:
          Boolean(buildDepOpts.ignoreScripts) ||
          !allowBuild(depGraph[depPath]?.name ?? ''),
      })
    })
  })

  await runGroups.default(opts.childConcurrency ?? 4, groups)
}

async function buildDependency(
  depPath: string,
  depGraph: GenericDependenciesGraph<DependenciesGraphNode>,
  opts: {
    extraBinPaths?: string[] | undefined
    extraNodePaths?: string[] | undefined
    extraEnv?: Record<string, string> | undefined
    depsStateCache: DepsStateCache
    ignoreScripts?: boolean | undefined
    lockfileDir: string
    optional?: boolean | undefined
    preferSymlinkedExecutables?: boolean | undefined
    rawConfig: object
    rootModulesDir: string
    scriptsPrependNodePath?: boolean | 'warn-only' | undefined
    scriptShell?: string | undefined
    shellEmulator?: boolean | undefined
    sideEffectsCacheWrite: boolean
    storeController: StoreController
    unsafePerm: boolean
    hoistedLocations?: Record<string, string[]> | undefined
    builtHoistedDeps?: Record<string, DeferredPromise<void>> | undefined
    warn: (message: string) => void
  }
): Promise<void> {
  const depNode = depGraph[depPath]

  if (!depNode?.filesIndexFile) {
    return
  }

  if (opts.builtHoistedDeps) {
    if (opts.builtHoistedDeps[depNode.depPath]) {
      await opts.builtHoistedDeps[depNode.depPath]?.promise

      return
    }

    opts.builtHoistedDeps[depNode.depPath] = pDefer()
  }

  try {
    await linkBinsOfDependencies(depNode, depGraph, opts)

    const isPatched = depNode.patchFile?.path != null

    if (isPatched) {
      applyPatchToDir({
        patchedDir: depNode.dir ?? '',
        patchFilePath: depNode.patchFile?.path ?? '',
      })
    }

    const hasSideEffects =
      !opts.ignoreScripts &&
      (await runPostinstallHooks({
        depPath,
        extraBinPaths: opts.extraBinPaths,
        extraEnv: opts.extraEnv,
        initCwd: opts.lockfileDir,
        optional: depNode.optional,
        pkgRoot: depNode.dir ?? '',
        rawConfig: opts.rawConfig,
        rootModulesDir: opts.rootModulesDir,
        scriptsPrependNodePath: opts.scriptsPrependNodePath,
        scriptShell: opts.scriptShell,
        shellEmulator: opts.shellEmulator,
        unsafePerm: opts.unsafePerm || false,
      }))

    if ((isPatched || hasSideEffects) && opts.sideEffectsCacheWrite) {
      try {
        const sideEffectsCacheKey = calcDepState(
          depGraph,
          opts.depsStateCache,
          depPath,
          {
            patchFileHash: depNode.patchFile?.hash,
            isBuilt: hasSideEffects,
          }
        )

        await opts.storeController.upload(depNode.dir ?? '', {
          sideEffectsCacheKey,
          filesIndexFile: depNode.filesIndexFile,
        })
      } catch (err: unknown) {
        // @ts-ignore
        if (err.statusCode === 403) {
          logger.warn({
            message: `The store server disabled upload requests, could not upload ${depNode.dir}`,
            prefix: opts.lockfileDir,
          })
        } else {
          logger.warn({
            // @ts-ignore
            error: err,
            message: `An error occurred while uploading ${depNode.dir}`,
            prefix: opts.lockfileDir,
          })
        }
      }
    }
  } catch (err: unknown) {
    if (depNode.optional) {
      // TODO: add parents field to the log
      const pkg = (await readPackageJsonFromDir(
        path.join(depNode.dir ?? '')
      )) as DependencyManifest
      skippedOptionalDependencyLogger.debug({
        // @ts-ignore
        details: err.toString(),
        package: {
          id: depNode.dir ?? '',
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
      const currentHoistedLocation = path.relative(
        opts.lockfileDir,
        depNode.dir ?? ''
      )

      const nonBuiltHoistedDeps = hoistedLocationsOfDep?.filter(
        (hoistedLocation) => hoistedLocation !== currentHoistedLocation
      )

      await hardLinkDir(depNode.dir ?? '', nonBuiltHoistedDeps)
    }

    if (opts.builtHoistedDeps) {
      opts.builtHoistedDeps[depNode.depPath]?.resolve()
    }
  }
}

export async function linkBinsOfDependencies(
  depNode: DependenciesGraphNode,
  depGraph: GenericDependenciesGraph<DependenciesGraphNode>,
  opts: {
    extraNodePaths?: string[] | undefined
    optional?: boolean | undefined
    preferSymlinkedExecutables?: boolean | undefined
    warn: (message: string) => void
  }
): Promise<void> {
  const childrenToLink: Record<string, string | undefined> | undefined = opts.optional
    ? depNode.children
    : pickBy.default(
      (_child, childAlias): boolean => {
        return !depNode.optionalDependencies?.has(childAlias);
      },
      depNode.children
    )

  const binPath = path.join(depNode.dir ?? '', 'node_modules/.bin')

  const pkgNodes = [
    ...Object.entries(childrenToLink ?? {})
      .map(([alias, childDepPath]): {
        alias: string;
        dep: DependenciesGraphNode | undefined;
      } => {
        return { alias, dep: depGraph[childDepPath ?? ''] };
      })
      .filter(({ alias, dep }: {
        alias: string;
        dep: DependenciesGraphNode | undefined;
      }): boolean => {
        if (!dep) {
          // TODO: Try to reproduce this issue with a test in @pnpm/core
          logger.debug({
            message: `Failed to link bins of "${alias}" to "${binPath}". This is probably not an issue.`,
          })

          return false
        }

        return dep.hasBin !== true || dep.installable !== false
      })
      .map(({ dep }: {
        alias: string;
        dep: DependenciesGraphNode | undefined;
      }): DependenciesGraphNode | undefined => {
        return dep;
      }).filter(Boolean),
    depNode,
  ]

  const pkgs = await Promise.all(
    pkgNodes.map(async (dep: DependenciesGraphNode): Promise<{
      location: string;
      manifest: PackageManifest | undefined;
    }> => {
      return {
        location: dep.dir ?? '',
        manifest:
          (await dep.fetchingBundledManifest?.()) ??
          ((await safeReadPackageJsonFromDir(dep.dir ?? ''))) ??
          undefined,
      };
    })
  )

  await linkBinsOfPackages(pkgs, binPath, {
    extraNodePaths: opts.extraNodePaths,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
  })

  // link also the bundled dependencies` bins
  if (depNode.hasBundledDependencies) {
    const bundledModules = path.join(depNode.dir ?? '', 'node_modules')

    await linkBins(bundledModules, binPath, {
      extraNodePaths: opts.extraNodePaths,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      warn: opts.warn,
    })
  }
}
