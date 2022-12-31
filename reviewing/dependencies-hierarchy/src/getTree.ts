import {
  PackageSnapshots,
} from '@pnpm/lockfile-file'
import { Registries } from '@pnpm/types'
import { refToRelative } from '@pnpm/dependency-path'
import { SearchFunction } from './types'
import { PackageNode } from './PackageNode'
import { getPkgInfo } from './getPkgInfo'
import { DependenciesCache } from './DependenciesCache'

interface GetTreeOpts {
  maxDepth: number
  modulesDir: string
  includeOptionalDependencies: boolean
  search?: SearchFunction
  skipped: Set<string>
  registries: Registries
  currentPackages: PackageSnapshots
  wantedPackages: PackageSnapshots
}

interface DependencyInfo {
  dependencies: PackageNode[]

  circular?: true

  /**
   * Whether or not the dependencies array was fully enumerated. This may not be
   * the case if a max depth was hit.
   */
  isPartiallyVisited: boolean

  /**
   * The number of edges along longest path. null if the dependencies array is
   * empty.
   */
  height: number | null
}

export function getTree (
  opts: GetTreeOpts,
  keypath: string[],
  parentId: string
): PackageNode[] {
  const dependenciesCache = new DependenciesCache()

  return getTreeHelper(dependenciesCache, opts, keypath, parentId).dependencies
}

function getTreeHelper (
  dependenciesCache: DependenciesCache,
  opts: GetTreeOpts,
  keypath: string[],
  parentId: string
): DependencyInfo {
  if (opts.maxDepth <= 0) {
    return { dependencies: [], isPartiallyVisited: true, height: null }
  }

  if (!opts.currentPackages?.[parentId]) {
    return { dependencies: [], isPartiallyVisited: false, height: null }
  }

  const deps = !opts.includeOptionalDependencies
    ? opts.currentPackages[parentId].dependencies
    : {
      ...opts.currentPackages[parentId].dependencies,
      ...opts.currentPackages[parentId].optionalDependencies,
    }

  if (deps == null) {
    return { dependencies: [], isPartiallyVisited: false, height: null }
  }

  const getChildrenTree = getTreeHelper.bind(null, dependenciesCache, {
    ...opts,
    maxDepth: opts.maxDepth - 1,
  })

  const peers = new Set(Object.keys(opts.currentPackages[parentId].peerDependencies ?? {}))

  const resultDependencies: PackageNode[] = []
  let resultHeight: number | null = null
  let resultCircular: boolean = false
  let resultIsPartiallyVisited = false

  Object.entries(deps).forEach(([alias, ref]) => {
    const { packageInfo, packageAbsolutePath } = getPkgInfo({
      alias,
      currentPackages: opts.currentPackages,
      modulesDir: opts.modulesDir,
      peers,
      ref,
      registries: opts.registries,
      skipped: opts.skipped,
      wantedPackages: opts.wantedPackages,
    })
    let circular: boolean
    const matchedSearched = opts.search?.(packageInfo)
    let newEntry: PackageNode | null = null
    if (packageAbsolutePath === null) {
      circular = false
      if (opts.search == null || matchedSearched) {
        newEntry = packageInfo
      }
    } else {
      let dependencies: PackageNode[] | undefined

      const relativeId = refToRelative(ref, alias) as string // we know for sure that relative is not null if pkgPath is not null
      circular = keypath.includes(relativeId)

      if (circular) {
        dependencies = []
      } else {
        const requestedDepth = opts.maxDepth
        dependencies = dependenciesCache.get({ packageAbsolutePath, requestedDepth })

        if (dependencies == null) {
          const children = getChildrenTree(keypath.concat([relativeId]), relativeId)
          dependencies = children.dependencies
          const heightOfCurrentDepNode = children.height == null ? 0 : children.height + 1
          resultHeight = Math.max(resultHeight ?? 0, heightOfCurrentDepNode)
          resultIsPartiallyVisited = resultIsPartiallyVisited || children.isPartiallyVisited

          if (children.circular) {
            resultCircular = true
          } else if (children.isPartiallyVisited) {
            dependenciesCache.addPartiallyVisitedResult(packageAbsolutePath, {
              dependencies,
              depth: requestedDepth,
            })
          } else {
            dependenciesCache.addFullyVisitedResult(packageAbsolutePath, {
              dependencies,
              height: heightOfCurrentDepNode,
            })
          }
        }
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
    isPartiallyVisited: resultIsPartiallyVisited,
    height: resultHeight,
  }

  if (resultCircular) {
    result.circular = resultCircular
  }

  return result
}
