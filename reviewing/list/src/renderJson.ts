import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import { type DependencyNode } from '@pnpm/reviewing.dependencies-hierarchy'
import { sortBy, path, type Ord } from 'ramda'
import { getPkgInfo, type PkgInfo } from './getPkgInfo.js'
import { type PackageDependencyHierarchy } from './types.js'

const sortPackages = sortBy(path(['pkg', 'alias']) as (pkg: DependencyNode) => Ord)

type RenderJsonResultItem = Pick<PackageDependencyHierarchy, 'name' | 'version' | 'path'> &
Required<Pick<PackageDependencyHierarchy, 'private'>> &
{
  dependencies?: Record<string, PackageJsonListItem>
  devDependencies?: Record<string, PackageJsonListItem>
  optionalDependencies?: Record<string, PackageJsonListItem>
  unsavedDependencies?: Record<string, PackageJsonListItem>
}

export async function renderJson (
  pkgs: PackageDependencyHierarchy[],
  opts: {
    depth: number
    long: boolean
    search: boolean
  }
): Promise<string> {
  const jsonArr = await Promise.all(pkgs.map(async (pkg) => {
    const jsonObj: RenderJsonResultItem = {
      name: pkg.name,
      version: pkg.version,
      path: pkg.path,
      private: !!pkg.private,
    }
    Object.assign(jsonObj,
      Object.fromEntries(
        await Promise.all(
          ([...DEPENDENCIES_FIELDS.sort(), 'unsavedDependencies'] as const)
            .filter((dependenciesField) => pkg[dependenciesField]?.length)
            .map(async (dependenciesField) => [
              dependenciesField,
              await toJsonResult(pkg[dependenciesField]!, { long: opts.long }),
            ]
            )
        )
      )
    )

    return jsonObj
  }))

  return JSON.stringify(jsonArr, null, 2)
}

export async function toJsonResult (
  entryNodes: DependencyNode[],
  opts: {
    long: boolean
  }
): Promise<Record<string, PackageJsonListItem>> {
  const dependencies: Record<string, PackageJsonListItem> = {}
  await Promise.all(
    sortPackages(entryNodes).map(async (node) => {
      const subDependencies = await toJsonResult(node.dependencies ?? [], opts)
      const dep: PackageJsonListItem = opts.long
        ? await getPkgInfo(node)
        : {
          alias: node.alias as string | undefined,
          from: node.name,
          version: node.version,
          resolved: node.resolved,
          path: node.path,
        }
      if (Object.keys(subDependencies).length > 0) {
        dep.dependencies = subDependencies
      }
      if (node.deduped) {
        dep.deduped = true
        if (node.dedupedDependenciesCount) {
          dep.dedupedDependenciesCount = node.dedupedDependenciesCount
        }
      }
      if (!dep.resolved) {
        delete dep.resolved
      }
      delete dep.alias
      dependencies[node.alias] = dep
    })
  )
  return dependencies
}

interface PackageJsonListItem extends PkgInfo {
  deduped?: true
  dedupedDependenciesCount?: number
  dependencies?: Record<string, PackageJsonListItem>
}
