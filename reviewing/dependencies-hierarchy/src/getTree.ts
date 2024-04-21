import path from 'path'
import { type PackageSnapshots, type ProjectSnapshot } from '@pnpm/lockfile-file'
import { type DepTypes } from '@pnpm/lockfile.detect-dep-types'
import { type Registries } from '@pnpm/types'
import { type SearchFunction } from './types'
import { type PackageNode } from './PackageNode'
import { getPkgInfo } from './getPkgInfo'
import { getTreeNodeChildId } from './getTreeNodeChildId'
import { DependenciesCache } from './DependenciesCache'
import { serializeTreeNodeId, type TreeNodeId } from './TreeNodeId'

interface GetTreeOpts {
  maxDepth: number
  rewriteLinkVersionDir: string
  includeOptionalDependencies: boolean
  lockfileDir: string
  onlyProjects?: boolean
  search?: SearchFunction
  skipped: Set<string>
  registries: Registries
  importers: Record<string, ProjectSnapshot>
  depTypes: DepTypes
  currentPackages: PackageSnapshots
  wantedPackages: PackageSnapshots
  virtualStoreDir?: string
}

interface DependencyInfo {
  dependencies: PackageNode[]

  circular?: true

  /**
   * The number of edges along the longest path, including the parent node.
   *
   *   - `"unknown"` if traversal was limited by a max depth option, therefore
   *      making the true height of a package undetermined.
   *   - `0` if the dependencies array is empty.
   *   - `1` if the dependencies array has at least 1 element and no child
   *     dependencies.
   */
  height: number | 'unknown'
}

export function getTree (
  opts: GetTreeOpts,
  parentId: TreeNodeId
): PackageNode[] {
  const dependenciesCache = new DependenciesCache()

  return getTreeHelper(dependenciesCache, opts, Keypath.initialize(parentId), parentId).dependencies
}

function getTreeHelper (
  dependenciesCache: DependenciesCache,
  opts: GetTreeOpts,
  keypath: Keypath,
  parentId: TreeNodeId
): DependencyInfo {
  if (opts.maxDepth <= 0) {
    return { dependencies: [], height: 'unknown' }
  }

  function getSnapshot (treeNodeId: TreeNodeId) {
    switch (treeNodeId.type) {
    case 'importer':
      return opts.importers[treeNodeId.importerId]
    case 'package':
      return opts.currentPackages[treeNodeId.depPath]
    }
  }

  const snapshot = getSnapshot(parentId)

  if (!snapshot) {
    return { dependencies: [], height: 0 }
  }

  const deps = !opts.includeOptionalDependencies
    ? snapshot.dependencies
    : {
      ...snapshot.dependencies,
      ...snapshot.optionalDependencies,
    }

  if (deps == null) {
    return { dependencies: [], height: 0 }
  }

  const childTreeMaxDepth = opts.maxDepth - 1
  const getChildrenTree = getTreeHelper.bind(null, dependenciesCache, {
    ...opts,
    maxDepth: childTreeMaxDepth,
  })

  function getPeerDependencies () {
    switch (parentId.type) {
    case 'importer':
      // Projects in the pnpm workspace can declare peer dependencies, but pnpm
      // doesn't record this block to the importers lockfile object. Returning
      // undefined for now.
      return undefined
    case 'package':
      return opts.currentPackages[parentId.depPath]?.peerDependencies
    }
  }
  const peers = new Set(Object.keys(getPeerDependencies() ?? {}))

  // If the "ref" of any dependency is a file system path (e.g. link:../), the
  // base directory of this relative path depends on whether the dependent
  // package is in the pnpm workspace or from node_modules.
  function getLinkedPathBaseDir () {
    switch (parentId.type) {
    case 'importer':
      return path.join(opts.lockfileDir, parentId.importerId)
    case 'package':
      return opts.lockfileDir
    }
  }
  const linkedPathBaseDir = getLinkedPathBaseDir()

  const resultDependencies: PackageNode[] = []
  let resultHeight: number | 'unknown' = 0
  let resultCircular: boolean = false

  Object.entries(deps).forEach(([alias, ref]) => {
    const packageInfo = getPkgInfo({
      alias,
      currentPackages: opts.currentPackages,
      depTypes: opts.depTypes,
      rewriteLinkVersionDir: opts.rewriteLinkVersionDir,
      linkedPathBaseDir,
      peers,
      ref,
      registries: opts.registries,
      skipped: opts.skipped,
      wantedPackages: opts.wantedPackages,
      virtualStoreDir: opts.virtualStoreDir,
    })
    let circular: boolean
    const matchedSearched = opts.search?.(packageInfo)
    let newEntry: PackageNode | null = null
    const nodeId = getTreeNodeChildId({
      parentId,
      dep: { alias, ref },
      lockfileDir: opts.lockfileDir,
      importers: opts.importers,
    })

    if (opts.onlyProjects && nodeId?.type !== 'importer') {
      return
    } else if (nodeId == null) {
      circular = false
      if (opts.search == null || matchedSearched) {
        newEntry = packageInfo
      }
    } else {
      let dependencies: PackageNode[] | undefined

      circular = keypath.includes(nodeId)

      if (circular) {
        dependencies = []
      } else {
        const cacheEntry = dependenciesCache.get({ parentId: nodeId, requestedDepth: childTreeMaxDepth })
        const children = cacheEntry ?? getChildrenTree(keypath.concat(nodeId), nodeId)

        if (cacheEntry == null && !children.circular) {
          if (children.height === 'unknown') {
            dependenciesCache.addPartiallyVisitedResult(nodeId, {
              dependencies: children.dependencies,
              depth: childTreeMaxDepth,
            })
          } else {
            dependenciesCache.addFullyVisitedResult(nodeId, {
              dependencies: children.dependencies,
              height: children.height,
            })
          }
        }

        const heightOfCurrentDepNode = children.height === 'unknown'
          ? 'unknown'
          : children.height + 1

        dependencies = children.dependencies
        resultHeight = resultHeight === 'unknown' || heightOfCurrentDepNode === 'unknown'
          ? 'unknown'
          : Math.max(resultHeight, heightOfCurrentDepNode)
        resultCircular = resultCircular || (children.circular ?? false)
      }

      if (dependencies.length > 0) {
        newEntry = {
          ...packageInfo,
          dependencies,
        }
      } else if ((opts.search == null) || matchedSearched) {
        newEntry = packageInfo
      }
    }
    if (newEntry != null) {
      if (circular) {
        newEntry.circular = true
        resultCircular = true
      }
      if (matchedSearched) {
        newEntry.searched = true
      }
      resultDependencies.push(newEntry)
    }
  })

  const result: DependencyInfo = {
    dependencies: resultDependencies,
    height: resultHeight,
  }

  if (resultCircular) {
    result.circular = resultCircular
  }

  return result
}

/**
 * Useful for detecting cycles.
 */
class Keypath {
  private constructor (private readonly keypath: readonly string[]) {}

  public static initialize (treeNodeId: TreeNodeId): Keypath {
    return new Keypath([serializeTreeNodeId(treeNodeId)])
  }

  public includes (treeNodeId: TreeNodeId): boolean {
    return this.keypath.includes(serializeTreeNodeId(treeNodeId))
  }

  public concat (treeNodeId: TreeNodeId): Keypath {
    return new Keypath([...this.keypath, serializeTreeNodeId(treeNodeId)])
  }
}
