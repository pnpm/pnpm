import {
  PackageSnapshots, ResolvedDependencies,
} from '@pnpm/lockfile-file'
import { Registries } from '@pnpm/types'
import { getPkgInfo } from './getPkgInfo'
import { PackageNode, SearchFunction } from './types'

export interface GetTreeOpts {
  currentDepth: number
  maxDepth: number
  lockfileDir: string
  modulesDir: string
  includeOptionalDependencies: boolean
  search?: SearchFunction
  skipped: Set<string>
  registries: Registries
  currentPackages: PackageSnapshots
  wantedPackages: PackageSnapshots
}

type PkgInfo = ReturnType<typeof getPkgInfo>

export function getTree (
  opts: GetTreeOpts,
  parentId: string
): PackageNode[] {
  function getResolvedDependencies (packageId: string): ResolvedDependencies | undefined {
    return !opts.includeOptionalDependencies
      ? opts.currentPackages[packageId].dependencies
      : {
        ...opts.currentPackages[packageId].dependencies,
        ...opts.currentPackages[packageId].optionalDependencies,
      }
  }

  function * traverseTree () {
    const dependenciesCache = new Map<string, PkgInfo>()

    function getPkgInfoCached (alias: string, peers: Set<string>, ref: string): PkgInfo {
      const cacheKey = JSON.stringify([alias, [...peers].sort(), ref])
      const existingItem = dependenciesCache.get(cacheKey)
      if (existingItem != null) {
        return existingItem
      }

      const pkgInfo = getPkgInfo({
        alias,
        currentPackages: opts.currentPackages,
        lockfileDir: opts.lockfileDir,
        modulesDir: opts.modulesDir,
        peers,
        ref,
        registries: opts.registries,
        skipped: opts.skipped,
        wantedPackages: opts.wantedPackages,
      })

      dependenciesCache.set(cacheKey, pkgInfo)
      return pkgInfo
    }

    interface StackItem {
      readonly keypath: readonly string[]
      readonly packageId: string
    }

    const stack: StackItem[] = [{ keypath: [], packageId: parentId }]

    let nextStackItem = stack.pop()
    while (nextStackItem !== undefined) {
      const { packageId, keypath } = nextStackItem

      const deps = getResolvedDependencies(packageId)
      if (deps == null) {
        continue
      }

      const nextKeypath = [...keypath, packageId]

      for (const [depName, resolution] of Object.entries(deps)) {
        const peers = new Set(Object.keys(opts.currentPackages[packageId].peerDependencies ?? {}))
        const { packageAbsolutePath, packageInfo } = getPkgInfoCached(depName, peers, resolution)

        if (packageAbsolutePath == null) {
          // TODO
          continue
        }

        const isCircular = nextKeypath.includes(packageAbsolutePath)
        if (isCircular) {
          // TODO: Something in the logic here is causing this to infinite loop
          // on circular dependencies.
          continue
        }

        yield {
          keypath: nextKeypath,
          packageAbsolutePath,
          packageInfo,
        }

        if (nextKeypath.length < opts.maxDepth) {
          stack.push({ keypath: nextKeypath, packageId: packageAbsolutePath })
        }
      }

      nextStackItem = stack.pop()
    }
  }

  const rootDependencies: PackageNode[] = []
  const mapKeypathToPackageNode: Map<string, PackageNode> = new Map()

  mapKeypathToPackageNode.set(JSON.stringify([parentId]), {} as PackageNode)

  for (const item of traverseTree()) {
    const parentNode = mapKeypathToPackageNode.get(JSON.stringify(item.keypath))
    if (parentNode == null) {
      throw new Error(`Failed to get parent node for: ${JSON.stringify(item.keypath)}`)
    }

    const parentDependencies = parentNode.dependencies ?? []
    parentNode.dependencies = parentDependencies

    const newEntry: PackageNode = { ...item.packageInfo }

    parentDependencies.push(newEntry)
    mapKeypathToPackageNode.set(JSON.stringify([...item.keypath, item.packageAbsolutePath]), newEntry)
    if (item.keypath.length === 1) {
      rootDependencies.push(newEntry)
    }
  }

  return rootDependencies
}
