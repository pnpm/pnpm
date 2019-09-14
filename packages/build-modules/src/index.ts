import { ENGINE_NAME } from '@pnpm/constants'
import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import { runPostinstallHooks } from '@pnpm/lifecycle'
import linkBins, { linkBinsOfPackages } from '@pnpm/link-bins'
import logger from '@pnpm/logger'
import { fromDir as readPackageFromDir } from '@pnpm/read-package-json'
import { StoreController } from '@pnpm/store-controller-types'
import { DependencyManifest, PackageManifest } from '@pnpm/types'
import graphSequencer = require('graph-sequencer')
import path = require('path')
import R = require('ramda')
import runGroups from 'run-groups'

export default async (
  depGraph: DependenciesGraph,
  rootDepPaths: string[],
  opts: {
    childConcurrency?: number,
    depsToBuild?: Set<string>,
    extraBinPaths?: string[],
    optional: boolean,
    prefix: string,
    rawNpmConfig: object,
    unsafePerm: boolean,
    userAgent: string,
    sideEffectsCacheWrite: boolean,
    storeController: StoreController,
    rootNodeModulesDir: string,
  },
) => {
  const warn = (message: string) => logger.warn({ message, prefix: opts.prefix })
  // postinstall hooks
  const nodesToBuild = new Set<string>()
  getSubgraphToBuild(depGraph, rootDepPaths, nodesToBuild, new Set<string>())
  const onlyFromBuildGraph = R.filter((depPath: string) => nodesToBuild.has(depPath))

  const nodesToBuildArray = Array.from(nodesToBuild)
  const graph = new Map(
    nodesToBuildArray
      .map((depPath) => [depPath, onlyFromBuildGraph(R.values(depGraph[depPath].children))]) as Array<[string, string[]]>,
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
      async () => buildDependency(depPath, depGraph, buildDepOpts)
    )
  })
  await runGroups(opts.childConcurrency || 4, groups)
}

async function buildDependency (
  depPath: string,
  depGraph: DependenciesGraph,
  opts: {
    extraBinPaths?: string[],
    optional: boolean,
    prefix: string,
    rawNpmConfig: object,
    rootNodeModulesDir: string,
    sideEffectsCacheWrite: boolean,
    storeController: StoreController,
    unsafePerm: boolean,
    warn: (message: string) => void,
  }
) {
  const depNode = depGraph[depPath]
  try {
    await linkBinsOfDependencies(depNode, depGraph, opts)
    const hasSideEffects = await runPostinstallHooks({
      depPath,
      extraBinPaths: opts.extraBinPaths,
      optional: depNode.optional,
      pkgRoot: depNode.peripheralLocation,
      prepare: depNode.prepare,
      rawNpmConfig: opts.rawNpmConfig,
      rootNodeModulesDir: opts.rootNodeModulesDir,
      unsafePerm: opts.unsafePerm || false,
    })
    if (hasSideEffects && opts.sideEffectsCacheWrite) {
      try {
        await opts.storeController.upload(depNode.peripheralLocation, {
          engine: ENGINE_NAME,
          packageId: depNode.packageId,
        })
      } catch (err) {
        if (err && err.statusCode === 403) {
          logger.warn({
            message: `The store server disabled upload requests, could not upload ${depNode.packageId}`,
            prefix: opts.prefix,
          })
        } else {
          logger.warn({
            error: err,
            message: `An error occurred while uploading ${depNode.packageId}`,
            prefix: opts.prefix,
          })
        }
      }
    }
  } catch (err) {
    if (depNode.optional) {
      // TODO: add parents field to the log
      const pkg = await readPackageFromDir(path.join(depNode.peripheralLocation)) as DependencyManifest
      skippedOptionalDependencyLogger.debug({
        details: err.toString(),
        package: {
          id: depNode.packageId,
          name: pkg.name,
          version: pkg.version,
        },
        prefix: opts.prefix,
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
  walked: Set<string>,
) {
  let currentShouldBeBuilt = false
  for (const depPath of entryNodes) {
    if (!graph[depPath]) return // packages that are already in node_modules are skipped
    if (nodesToBuild.has(depPath)) {
      currentShouldBeBuilt = true
    }
    if (walked.has(depPath)) continue
    walked.add(depPath)
    const childShouldBeBuilt = getSubgraphToBuild(graph, R.values(graph[depPath].children), nodesToBuild, walked)
      || graph[depPath].requiresBuild
    if (childShouldBeBuilt) {
      nodesToBuild.add(depPath)
      currentShouldBeBuilt = true
    }
  }
  return currentShouldBeBuilt
}

export interface DependenciesGraphNode {
  fetchingBundledManifest?: () => Promise<PackageManifest>,
  hasBundledDependencies: boolean,
  peripheralLocation: string,
  children: {[alias: string]: string},
  optional: boolean,
  optionalDependencies: Set<string>,
  packageId: string, // TODO: this option is currently only needed when running postinstall scripts but even there it should be not used
  installable?: boolean,
  isBuilt?: boolean,
  requiresBuild?: boolean,
  prepare: boolean,
  hasBin: boolean,
}

export interface DependenciesGraph {
  [depPath: string]: DependenciesGraphNode
}

export async function linkBinsOfDependencies (
  depNode: DependenciesGraphNode,
  depGraph: DependenciesGraph,
  opts: {
    optional: boolean,
    warn: (message: string) => void,
  },
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

  const binPath = path.join(depNode.peripheralLocation, 'node_modules', '.bin')

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
          location: dep.peripheralLocation,
          manifest: dep.fetchingBundledManifest && (await dep.fetchingBundledManifest()) || (await readPackageFromDir(dep.peripheralLocation) as DependencyManifest),
        }
      }),
  )

  await linkBinsOfPackages(pkgs, binPath, { warn: opts.warn })

  // link also the bundled dependencies` bins
  if (depNode.hasBundledDependencies) {
    const bundledModules = path.join(depNode.peripheralLocation, 'node_modules')
    await linkBins(bundledModules, binPath, { warn: opts.warn })
  }
}
