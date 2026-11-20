import assert from 'node:assert'

import { calcDepState } from '@pnpm/deps.graph-hasher'
import { isError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import { publishBuiltSharedSideEffects } from '@pnpm/pnpr.client'
import type { UploadPkgToStoreResult } from '@pnpm/store.controller-types'

import type { BuildDependencyOptions } from './buildDependency.js'
import type { DependenciesGraph, DependenciesGraphNode } from './buildGraph.js'

export interface BuildOutput<NodeId extends string> {
  depPath: NodeId
  depGraph: DependenciesGraph<NodeId>
  /** The node whose directory holds the build output. See `buildPackage`. */
  pkgNode: DependenciesGraphNode<NodeId>
  opts: BuildDependencyOptions
  isPatched: boolean
  hasSideEffects: boolean
}

/** Caches a build's output in the side-effects cache. An upload that fails is reported, not thrown. */
export async function storeBuildOutput<NodeId extends string> (output: BuildOutput<NodeId>): Promise<void> {
  const { pkgNode: depNode, opts, isPatched, hasSideEffects } = output
  // frozenStore opens the store read-only, so the side-effects cache (which
  // lives in the store) cannot be written. extendInstallOptions already forces
  // sideEffectsCacheWrite off under frozenStore; this guards callers that
  // bypass it.
  const shouldPublishSharedSideEffects = hasSideEffects && sharesSideEffectsOf(depNode, opts)
  if (!isPatched && !hasSideEffects) return
  if (!(opts.sideEffectsCacheWrite || shouldPublishSharedSideEffects) || opts.frozenStore) return
  try {
    await uploadBuildOutput(output, shouldPublishSharedSideEffects)
  } catch (err: unknown) {
    assert(isError(err))
    logger.warn({
      error: err,
      message: `An error occurred while uploading ${depNode.dir}`,
      prefix: opts.lockfileDir,
    })
  }
}

async function uploadBuildOutput<NodeId extends string> (
  output: BuildOutput<NodeId>,
  shouldPublishSharedSideEffects: boolean
): Promise<void> {
  const { depPath, depGraph, pkgNode: depNode, opts, hasSideEffects } = output
  const sideEffectsCacheKey = calcDepState(depGraph, opts.depsStateCache, depPath, {
    patchFileHash: depNode.patch?.hash,
    includeDepGraphHash: hasSideEffects,
    nodeVersion: opts.nodeVersion,
  })
  if (!depNode.filesIndexFile) return
  const upload = await opts.storeController.upload(depNode.dir, {
    sideEffectsCacheKey,
    filesIndexFile: depNode.filesIndexFile,
  })
  if (shouldPublishSharedSideEffects && depNode.resolution != null) {
    await publishSideEffects({ depPath, depGraph, opts, resolution: depNode.resolution, upload })
  }
}

function sharesSideEffectsOf<NodeId extends string> (depNode: DependenciesGraphNode<NodeId>, opts: BuildDependencyOptions): boolean {
  return opts.remoteSideEffectsCache?.publish === true &&
    opts.pnprServer != null &&
    opts.remoteSideEffectsCache?.packages?.includes(depNode.name) === true &&
    depNode.resolution != null
}

async function publishSideEffects<NodeId extends string> (
  { depPath, depGraph, opts, resolution, upload }: Pick<BuildOutput<NodeId>, 'depPath' | 'depGraph' | 'opts'> & {
    resolution: NonNullable<DependenciesGraphNode<NodeId>['resolution']>
    upload: UploadPkgToStoreResult
  }
): Promise<void> {
  const depNode = depGraph[depPath]
  await publishBuiltSharedSideEffects({
    configByUri: opts.configByUri ?? {},
    depsGraph: depGraph,
    graphKey: depPath,
    name: depNode.name,
    nodeVersion: opts.nodeVersion,
    patchFileHash: depNode.patch?.hash,
    pnprServer: opts.pnprServer,
    resolution,
    settings: opts.remoteSideEffectsCache,
    supportedArchitectures: opts.supportedArchitectures,
    upload,
    version: depNode.version,
  })
}
