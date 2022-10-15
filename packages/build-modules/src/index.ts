import path from 'path'
import { calcDepState, DepsStateCache } from '@pnpm/calc-dep-state'
import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import { PnpmError } from '@pnpm/error'
import { runPostinstallHooks } from '@pnpm/lifecycle'
import { linkBins, linkBinsOfPackages } from '@pnpm/link-bins'
import { logger } from '@pnpm/logger'
import { readPackageJsonFromDir, safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { StoreController } from '@pnpm/store-controller-types'
import { DependencyManifest } from '@pnpm/types'
import { applyPatch } from 'patch-package/dist/applyPatches'
import runGroups from 'run-groups'
import { buildSequence, DependenciesGraph, DependenciesGraphNode } from './buildSequence'

export { DepsStateCache }

export async function buildModules (
  depGraph: DependenciesGraph,
  rootDepPaths: string[],
  opts: {
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
  }
) {
  const warn = (message: string) => logger.warn({ message, prefix: opts.lockfileDir })
  // postinstall hooks

  const buildDepOpts = { ...opts, warn }
  const chunks = buildSequence(depGraph, rootDepPaths)
  const groups = chunks.map((chunk) => {
    chunk = chunk.filter((depPath) => {
      const node = depGraph[depPath]
      return (node.requiresBuild || node.patchFile != null) && !node.isBuilt
    })
    if (opts.depsToBuild != null) {
      chunk = chunk.filter((depPath) => opts.depsToBuild!.has(depPath))
    }

    return chunk.map((depPath: string) =>
      async () => buildDependency(depPath, depGraph, buildDepOpts)
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
    warn: (message: string) => void
  }
) {
  const depNode = depGraph[depPath]
  try {
    await linkBinsOfDependencies(depNode, depGraph, opts)
    const isPatched = depNode.patchFile?.path != null
    if (isPatched) {
      applyPatchToDep(depNode.dir, depNode.patchFile!.path)
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
  }
}

function applyPatchToDep (patchDir: string, patchFilePath: string) {
  // Ideally, we would just run "patch" or "git apply".
  // However, "patch" is not available on Windows and "git apply" is hard to execute on a subdirectory of an existing repository
  const cwd = process.cwd()
  process.chdir(patchDir)
  const success = applyPatch({
    patchFilePath,
    patchDir,
  })
  process.chdir(cwd)
  if (!success) {
    throw new PnpmError('PATCH_FAILED', `Could not apply patch ${patchFilePath} to ${patchDir}`)
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
  const childrenToLink = opts.optional
    ? depNode.children
    : Object.keys(depNode.children)
      .reduce((nonOptionalChildren, childAlias) => {
        if (!depNode.optionalDependencies.has(childAlias)) {
          nonOptionalChildren[childAlias] = depNode.children[childAlias]
        }
        return nonOptionalChildren
      }, {})

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
