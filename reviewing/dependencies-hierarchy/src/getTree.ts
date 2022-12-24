import {
  PackageSnapshots,
} from '@pnpm/lockfile-file'
import { Registries } from '@pnpm/types'
import { refToRelative } from '@pnpm/dependency-path'
import { SearchFunction } from './types'
import { PackageNode } from './PackageNode'
import { getPkgInfo } from './getPkgInfo'

interface GetTreeOpts {
  currentDepth: number
  maxDepth: number
  modulesDir: string
  includeOptionalDependencies: boolean
  search?: SearchFunction
  skipped: Set<string>
  registries: Registries
  currentPackages: PackageSnapshots
  wantedPackages: PackageSnapshots
}

interface DependencyInfo { circular?: true, dependencies: PackageNode[] }

export function getTree (
  opts: GetTreeOpts,
  keypath: string[],
  parentId: string
): PackageNode[] {
  const dependenciesCache = new Map<string, PackageNode[]>()

  return getTreeHelper(dependenciesCache, opts, keypath, parentId).dependencies
}

function getTreeHelper (
  dependenciesCache: Map<string, PackageNode[]>,
  opts: GetTreeOpts,
  keypath: string[],
  parentId: string
): DependencyInfo {
  const result: DependencyInfo = { dependencies: [] }
  if (opts.currentDepth > opts.maxDepth || !opts.currentPackages || !opts.currentPackages[parentId]) return result

  const deps = !opts.includeOptionalDependencies
    ? opts.currentPackages[parentId].dependencies
    : {
      ...opts.currentPackages[parentId].dependencies,
      ...opts.currentPackages[parentId].optionalDependencies,
    }

  if (deps == null) return result

  const getChildrenTree = getTreeHelper.bind(null, dependenciesCache, {
    ...opts,
    currentDepth: opts.currentDepth + 1,
  })

  const peers = new Set(Object.keys(opts.currentPackages[parentId].peerDependencies ?? {}))

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
        dependencies = dependenciesCache.get(packageAbsolutePath)

        if (dependencies == null) {
          const children = getChildrenTree(keypath.concat([relativeId]), relativeId)
          dependencies = children.dependencies

          if (children.circular) {
            result.circular = true
          } else {
            dependenciesCache.set(packageAbsolutePath, dependencies)
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
        result.circular = true
      }
      if (matchedSearched) {
        newEntry.searched = true
      }
      result.dependencies.push(newEntry)
    }
  })

  return result
}
