// TODO: move to separate package. It is used in supi/lib/install.ts as well

import {runPostinstallHooks} from '@pnpm/lifecycle'
import logger from '@pnpm/logger'
import graphSequencer = require('graph-sequencer')
import pLimit = require('p-limit')
import {StoreController} from 'package-store'
import path = require('path')
import R = require('ramda')
import readPkgCB = require('read-package-json')
import {skippedOptionalDependencyLogger} from 'supi/lib/loggers'
import promisify = require('util.promisify')
import {DepGraphNodesByDepPath} from '.'
import {ENGINE_NAME} from './constants'

const readPkg = promisify(readPkgCB)

export default async (
  depGraph: DepGraphNodesByDepPath,
  rootDepPaths: string[],
  opts: {
    childConcurrency?: number,
    prefix: string,
    rawNpmConfig: object,
    unsafePerm: boolean,
    userAgent: string,
    sideEffectsCache: boolean,
    sideEffectsCacheReadonly: boolean,
    storeController: StoreController,
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
              pkgRoot: depNode.peripheralLocation,
              rawNpmConfig: opts.rawNpmConfig,
              rootNodeModulesDir: opts.prefix,
              unsafePerm: opts.unsafePerm || false,
            })
            if (hasSideEffects && opts.sideEffectsCache && !opts.sideEffectsCacheReadonly) {
              try {
                await opts.storeController.upload(depNode.peripheralLocation, {
                  engine: ENGINE_NAME,
                  pkgId: depNode.pkgId,
                })
              } catch (err) {
                if (err && err.statusCode === 403) {
                  logger.warn(`The store server disabled upload requests, could not upload ${depNode.pkgId}`)
                } else {
                  logger.warn({
                    err,
                    message: `An error occurred while uploading ${depNode.pkgId}`,
                  })
                }
              }
            }
          } catch (err) {
            if (depNode.optional) {
              // TODO: add parents field to the log
              const pkg = await readPkg(path.join(depNode.peripheralLocation, 'package.json'))
              skippedOptionalDependencyLogger.debug({
                details: err.toString(),
                package: {
                  id: depNode.pkgId,
                  name: pkg.name,
                  version: pkg.version,
                },
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
  graph: DepGraphNodesByDepPath,
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
