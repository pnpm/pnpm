// TODO: move to separate package. It is used in supi/lib/install.ts as well

import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import { runPostinstallHooks } from '@pnpm/lifecycle'
import logger from '@pnpm/logger'
import { fromDir as readPackageFromDir } from '@pnpm/read-package-json'
import { StoreController } from '@pnpm/store-controller-types'
import graphSequencer = require('graph-sequencer')
import pLimit = require('p-limit')
import path = require('path')
import R = require('ramda')
import { DependenciesGraph } from '.'
import { ENGINE_NAME } from './constants'

export default async (
  depGraph: DependenciesGraph,
  rootDepPaths: string[],
  opts: {
    childConcurrency?: number,
    prefix: string,
    rawNpmConfig: object,
    unsafePerm: boolean,
    userAgent: string,
    sideEffectsCacheWrite: boolean,
    storeController: StoreController,
    rootNodeModulesDir: string,
  },
) => {
  // postinstall hooks
  const limitChild = pLimit(opts.childConcurrency || 4)

  const depPaths = Object.keys(depGraph)
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

  for (const chunk of chunks) {
    await Promise.all(chunk
      .filter((depPath) => depGraph[depPath].requiresBuild && !depGraph[depPath].isBuilt)
      .map((depPath: string) => limitChild(async () => {
        const depNode = depGraph[depPath]
        try {
          const hasSideEffects = await runPostinstallHooks({
            depPath,
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
                pkgId: depNode.pkgId,
              })
            } catch (err) {
              if (err && err.statusCode === 403) {
                logger.warn({
                  message: `The store server disabled upload requests, could not upload ${depNode.pkgId}`,
                  prefix: opts.prefix,
                })
              } else {
                logger.warn({
                  error: err,
                  message: `An error occurred while uploading ${depNode.pkgId}`,
                  prefix: opts.prefix,
                })
              }
            }
          }
        } catch (err) {
          if (depNode.optional) {
            // TODO: add parents field to the log
            const pkg = await readPackageFromDir(path.join(depNode.peripheralLocation))
            skippedOptionalDependencyLogger.debug({
              details: err.toString(),
              package: {
                id: depNode.pkgId,
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
      })))
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
