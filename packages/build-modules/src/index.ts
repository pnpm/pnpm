import { ENGINE_NAME } from '@pnpm/constants'
import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import { runPostinstallHooks } from '@pnpm/lifecycle'
import linkBins, { linkBinsOfPackages } from '@pnpm/link-bins'
import logger from '@pnpm/logger'
import { fromDir as readPackageFromDir } from '@pnpm/read-package-json'
import { StoreController } from '@pnpm/store-controller-types'
import { DependencyManifest, PackageManifest } from '@pnpm/types'
import runGroups from 'run-groups'
import path = require('path')
import graphSequencer = require('graph-sequencer')
import R = require('ramda')

export default async (
  depGraph: DependenciesGraph,
  rootDepPaths: string[],
  opts: {
    childConcurrency?: number
    depsToBuild?: Set<string>
    extraBinPaths?: string[]
    extraEnv?: Record<string, string>
    lockfileDir: string
    optional: boolean
    rawConfig: object
    unsafePerm: boolean
    userAgent: string
    scriptShell?: string
    shellEmulator?: boolean
    sideEffectsCacheWrite: boolean
    storeController: StoreController
    rootModulesDir: string
  }
) => {
  const warn = (message: string) => logger.warn({ message, prefix: opts.lockfileDir })
  // postinstall hooks
  const nodesToBuild = new Set<string>()
  getSubgraphToBuild(depGraph, rootDepPaths, nodesToBuild, new Set<string>())
  const onlyFromBuildGraph = R.filter((depPath: string) => nodesToBuild.has(depPath))

  const nodesToBuildArray = Array.from(nodesToBuild)
  const graph = new Map(
    nodesToBuildArray
      .map((depPath) => [depPath, onlyFromBuildGraph(R.values(depGraph[depPath].children))])
  )
  const graphSequencerResult = graphSequencer({
    graph,
    groups: [nodesToBuildArray],
  })
  const chunks = graphSequencerResult.chunks as string[][]
  const buildDepOpts = { ...opts, warn }
  const groups = chunks.map((chunk) => {
    chunk = chunk.filter((depPath) => depGraph[depPath].requiresBuild && !depGraph[depPath].isBuilt)
    if (opts.depsToBuild) {
      chunk = chunk.filter((depPath) => opts.depsToBuild!.has(depPath))
    }

    return chunk.map((depPath: string) =>
      () => buildDependency(depPath, depGraph, buildDepOpts)
    )
  })
  await runGroups(opts.childConcurrency ?? 4, groups)
}

async function buildDependency (
  depPath: string,
  depGraph: DependenciesGraph,
  opts: {
    extraBinPaths?: string[]
    extraEnv?: Record<string, string>
    lockfileDir: string
    optional: boolean
    rawConfig: object
    rootModulesDir: string
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
    const hasSideEffects = await runPostinstallHooks({
      depPath,
      extraBinPaths: opts.extraBinPaths,
      extraEnv: opts.extraEnv,
      initCwd: opts.lockfileDir,
      optional: depNode.optional,
      pkgRoot: depNode.dir,
      prepare: depNode.prepare,
      rawConfig: opts.rawConfig,
      rootModulesDir: opts.rootModulesDir,
      scriptShell: opts.scriptShell,
      shellEmulator: opts.shellEmulator,
      unsafePerm: opts.unsafePerm || false,
    })
    if (hasSideEffects && opts.sideEffectsCacheWrite) {
      try {
        await opts.storeController.upload(depNode.dir, {
          engine: ENGINE_NAME,
          filesIndexFile: depNode.filesIndexFile,
        })
      } catch (err) {
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
  } catch (err) {
    if (depNode.optional) {
      // TODO: add parents field to the log
      const pkg = await readPackageFromDir(path.join(depNode.dir)) as DependencyManifest
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

function getSubgraphToBuild (
  graph: DependenciesGraph,
  entryNodes: string[],
  nodesToBuild: Set<string>,
  walked: Set<string>
) {
  let currentShouldBeBuilt = false
  for (const depPath of entryNodes) {
    if (!graph[depPath]) return // packages that are already in node_modules are skipped
    if (nodesToBuild.has(depPath)) {
      currentShouldBeBuilt = true
    }
    if (walked.has(depPath)) continue
    walked.add(depPath)
    const childShouldBeBuilt = getSubgraphToBuild(graph, R.values(graph[depPath].children), nodesToBuild, walked) === true ||
      graph[depPath].requiresBuild
    if (childShouldBeBuilt) {
      nodesToBuild.add(depPath)
      currentShouldBeBuilt = true
    }
  }
  return currentShouldBeBuilt
}

export interface DependenciesGraphNode {
  children: {[alias: string]: string}
  dir: string
  fetchingBundledManifest?: () => Promise<PackageManifest>
  filesIndexFile: string
  hasBin: boolean
  hasBundledDependencies: boolean
  installable?: boolean
  isBuilt?: boolean
  optional: boolean
  optionalDependencies: Set<string>
  prepare: boolean
  requiresBuild?: boolean
}

export interface DependenciesGraph {
  [depPath: string]: DependenciesGraphNode
}

export async function linkBinsOfDependencies (
  depNode: DependenciesGraphNode,
  depGraph: DependenciesGraph,
  opts: {
    optional: boolean
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

  const pkgs = await Promise.all(
    Object.keys(childrenToLink)
      .filter((alias) => {
        const dep = depGraph[childrenToLink[alias]]
        if (!dep) {
          // TODO: Try to reproduce this issue with a test in supi
          logger.debug({ message: `Failed to link bins of "${alias}" to "${binPath}". This is probably not an issue.` })
          return false
        }
        return dep.hasBin && dep.installable !== false
      })
      .map(async (alias) => {
        const dep = depGraph[childrenToLink[alias]]
        return {
          location: dep.dir,
          manifest: await dep.fetchingBundledManifest?.() ?? (await readPackageFromDir(dep.dir) as DependencyManifest),
        }
      })
  )

  await linkBinsOfPackages(pkgs, binPath, { warn: opts.warn })

  // link also the bundled dependencies` bins
  if (depNode.hasBundledDependencies) {
    const bundledModules = path.join(depNode.dir, 'node_modules')
    await linkBins(bundledModules, binPath, { warn: opts.warn })
  }
}
