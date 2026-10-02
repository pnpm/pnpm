import { engineName } from '@pnpm/engine.runtime.system-version'
import type { SupportedArchitectures } from '@pnpm/types'

import { calcDepGraphHash, createDepGraphHashContext } from './calcDepGraphHash.js'
import { readSnapshotRuntimePin } from './readSnapshotRuntimePin.js'
import type { DepsGraph, DepsStateCache } from './types.js'

export const DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX = 'dependency-side-effects:v1:'

export interface CalcDepStateInputKeyOptions<NodeId extends string> {
  depsGraph: DepsGraph<NodeId>
  depPath: NodeId
  patchFileHash?: string
  supportedArchitectures?: SupportedArchitectures
}

/**
 * Compute the machine-independent lookup key for a remotely shareable
 * dependency build.
 *
 * depsGraph must contain depPath, and every reachable node must provide
 * either fullPkgId or the resolution metadata needed to derive it. The
 * function does not mutate the graph or caller state. Each call uses an
 * isolated cache, so the result is independent of earlier roots and platform
 * selections.
 *
 * The returned key starts with {@link DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX},
 * followed by the recursive dependency-graph hash and, when non-empty, the
 * patch-file hash. Host identity is excluded and advertised by the artifact's
 * signed compatibility constraints. When a resolution contains platform
 * variations, supportedArchitectures selects the source integrity included
 * in the graph hash.
 */
export function calcDepStateInputKey<NodeId extends string> (
  opts: CalcDepStateInputKeyOptions<NodeId>
): string {
  if (opts.depsGraph[opts.depPath] == null) {
    throw new Error(`Dependency side-effects input-key root ${opts.depPath} is not present in depsGraph`)
  }
  const depGraphHash = calcDepGraphHash({
    depsGraph: opts.depsGraph,
    cache: {},
    parents: new Set(),
    depPath: opts.depPath,
    context: createDepGraphHashContext(opts.supportedArchitectures),
  })
  let result = `${DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX}deps=${depGraphHash}`
  if (opts.patchFileHash) {
    result += `;patch=${opts.patchFileHash}`
  }
  return result
}

/**
 * The side-effects diff format the cache key names. Format 2 records the
 * symlinks a build creates, which a pnpm version reading format 1 would
 * restore as regular files, so the two formats are kept under separate keys.
 */
export const SIDE_EFFECTS_FORMAT_KEY = 'format=2'

export function calcDepState<NodeId extends string> (
  depsGraph: DepsGraph<NodeId>,
  cache: DepsStateCache,
  depPath: string,
  opts: {
    patchFileHash?: string
    includeDepGraphHash: boolean
    supportedArchitectures?: SupportedArchitectures
    /**
     * Install-wide fallback engines.runtime / devEngines.runtime
     * Node version (e.g. "22.11.0"). Used only when the snapshot at
     * depPath doesn't itself pin a Node: per-snapshot pins take
     * precedence so the side-effects-cache key reflects the actual
     * script-runner Node the bin linker would spawn for the package
     * (see {@link readSnapshotRuntimePin}).
     */
    nodeVersion?: string
  }
): string {
  const ownPin = readSnapshotRuntimePin(depsGraph[depPath as NodeId]?.children)
  let result = `${engineName(ownPin ?? opts.nodeVersion)};${SIDE_EFFECTS_FORMAT_KEY}`
  if (opts.includeDepGraphHash) {
    const depGraphHash = calcDepGraphHash({
      depsGraph,
      cache,
      parents: new Set(),
      depPath: depPath as NodeId,
      context: createDepGraphHashContext(opts.supportedArchitectures),
    })
    result += `;deps=${depGraphHash}`
  }
  if (opts.patchFileHash) {
    result += `;patch=${opts.patchFileHash}`
  }
  return result
}

export function shouldIncludeDepGraphHash (opts: {
  ignoreScripts: boolean
  deferDependencyBuilds: boolean
  requiresBuild: boolean | undefined
}): boolean {
  return (!opts.ignoreScripts || opts.deferDependencyBuilds) && opts.requiresBuild === true
}
