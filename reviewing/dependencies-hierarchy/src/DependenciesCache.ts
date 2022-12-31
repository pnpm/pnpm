import { PackageNode } from './PackageNode'

export interface GetDependenciesCacheEntryArgs {
  readonly packageAbsolutePath: string
  readonly requestedDepth: number
}

export interface TraversalResultFullyVisited {
  readonly dependencies: PackageNode[]

  /**
   * Describes the height of the parent node in the fully enumerated dependency
   * tree. A height of 0 means no entries are present in the dependencies array.
   * A height of 1 means entries in the dependencies array do not have any of
   * their own dependencies.
   */
  readonly height: number
}

export interface TraversalResultPartiallyVisited {
  readonly dependencies: PackageNode[]

  /**
   * Describes how deep the dependencies tree was previously traversed. Since
   * the traversal result was limited by a max depth, there are likely more
   * dependencies present deeper in the tree not shown.
   *
   * A depth of 0 would indicate no entries in the dependencies array. A depth
   * of 1 means entries in the dependencies array do not have any of their own
   * dependencies.
   */
  readonly depth: number
}

export interface CacheHit {
  readonly dependencies: PackageNode[]
  readonly height: number | 'unknown'
  // Circular dependencies are not stored in the cache.
  readonly circular: false
}

/**
 * A cache for the dependencies of a package.
 *
 * ## Depth Considerations
 *
 * Since the enumerated dependency tree can be limited by a max depth argument,
 * several considerations have to be made when caching.
 *
 *   - If a package is visited with a requested depth greater than the cached
 *     depth, the cache cannot be used. The tree needs to be enumerated again
 *     deeper.
 *   - If a package is visited with a requested depth less than the cached
 *     depth, the cache probably can't be used. This depends on how strict the
 *     depth constraint is and whether it's acceptable to exceed the max depth.
 *     This cache assumes the max depth should not be exceeded.
 *   - Cycles may or may not be cached. It depends on whether the cycle is
 *     introduced by a package outside of the cached tree.
 *
 * This cache adds an optimization when a dependency tree has been fully
 * enumerated and wasn't limited by a max depth argument. In that case,
 * dependency trees cached can be used when the max depth argument is greater
 * than or equal to the height of the tree root.
 *
 * ## Future Optimizations
 *
 * The necessity of this cache may be removed in the future with a refactor of
 * the `pnpm list` command. This cache attempts to optimize runtime to O(# of
 * unique packages), but the list command is O(# of nodes) anyway since every
 * node needs to be printed. It's possible a generator function could be
 * returned here to avoid computing large trees in-memory before passing to
 * downstream commands.
 */
export class DependenciesCache {
  private readonly fullyVisitedCache = new Map<string, TraversalResultFullyVisited>()

  /**
   *  Maps packageAbsolutePath -> visitedDepth -> dependencies
   */
  private readonly partiallyVisitedCache = new Map<string, Map<number, PackageNode[]>>()

  public get (args: GetDependenciesCacheEntryArgs): CacheHit | undefined {
    // The fully visited cache is only usable if the height doesn't exceed the
    // requested depth. Otherwise the final dependencies listing will print
    // entries with a greater depth than requested.
    //
    // If that is the case, the partially visited cache should be checked to see
    // if dependencies were requested at that exact depth before.
    const fullyVisitedEntry = this.fullyVisitedCache.get(args.packageAbsolutePath)
    if (fullyVisitedEntry !== undefined && fullyVisitedEntry.height <= args.requestedDepth) {
      return {
        dependencies: fullyVisitedEntry.dependencies,
        height: fullyVisitedEntry.height,
        circular: false,
      }
    }

    const partiallyVisitedEntry = this.partiallyVisitedCache.get(args.packageAbsolutePath)?.get(args.requestedDepth)
    if (partiallyVisitedEntry != null) {
      return {
        dependencies: partiallyVisitedEntry,
        height: 'unknown',
        circular: false,
      }
    }

    return undefined
  }

  public addFullyVisitedResult (packageAbsolutePath: string, result: TraversalResultFullyVisited): void {
    this.fullyVisitedCache.set(packageAbsolutePath, result)
  }

  public addPartiallyVisitedResult (packageAbsolutePath: string, result: TraversalResultPartiallyVisited): void {
    const dependenciesByDepth = this.partiallyVisitedCache.get(packageAbsolutePath) ?? new Map()
    if (!this.partiallyVisitedCache.has(packageAbsolutePath)) {
      this.partiallyVisitedCache.set(packageAbsolutePath, dependenciesByDepth)
    }

    dependenciesByDepth.set(result.depth, result.dependencies)
  }
}
