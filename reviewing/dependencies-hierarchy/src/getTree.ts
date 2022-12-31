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
    return { dependencies: [], height: 'unknown' }
  }

  if (!opts.currentPackages?.[parentId]) {
    return { dependencies: [], height: 0 }
  }

  const deps = !opts.includeOptionalDependencies
    ? opts.currentPackages[parentId].dependencies
    : {
      ...opts.currentPackages[parentId].dependencies,
      ...opts.currentPackages[parentId].optionalDependencies,
    }

  if (deps == null) {
    return { dependencies: [], height: 0 }
  }

  const childTreeMaxDepth = opts.maxDepth - 1
  const getChildrenTree = getTreeHelper.bind(null, dependenciesCache, {
    ...opts,
    maxDepth: childTreeMaxDepth,
  })

  const peers = new Set(Object.keys(opts.currentPackages[parentId].peerDependencies ?? {}))

  const resultDependencies: PackageNode[] = []
  let resultHeight: number | 'unknown' = 0
  let resultCircular: boolean = false

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
        const cacheEntry = dependenciesCache.get({ packageAbsolutePath, requestedDepth: childTreeMaxDepth })
        const children = cacheEntry ?? getChildrenTree(keypath.concat([relativeId]), relativeId)

        if (cacheEntry == null && !children.circular) {
          if (children.height === 'unknown') {
            dependenciesCache.addPartiallyVisitedResult(packageAbsolutePath, {
              dependencies: children.dependencies,
              depth: childTreeMaxDepth,
            })
          } else {
            dependenciesCache.addFullyVisitedResult(packageAbsolutePath, {
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
