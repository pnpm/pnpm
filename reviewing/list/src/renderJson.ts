import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import { PackageNode } from '@pnpm/reviewing.dependencies-hierarchy'
import sortBy from 'ramda/src/sortBy'
import path from 'ramda/src/path'
import { Ord } from 'ramda'
import { getPkgInfo, PkgInfo } from './getPkgInfo'
import { PackageDependencyHierarchy } from './types'

const sortPackages = sortBy(path(['pkg', 'alias']) as (pkg: PackageNode) => Ord)

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
    for (const dependenciesField of [...DEPENDENCIES_FIELDS.sort(), 'unsavedDependencies'] as const) {
      if (pkg[dependenciesField]?.length) {
        jsonObj[dependenciesField] = await toJsonResult(pkg[dependenciesField]!, { long: opts.long })
      }
    }

    return jsonObj
  }))

  return JSON.stringify(jsonArr, null, 2)
}

export async function toJsonResult (
  entryNodes: PackageNode[],
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
        }
      if (Object.keys(subDependencies).length > 0) {
        dep.dependencies = subDependencies
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
  dependencies?: Record<string, PackageJsonListItem>
}
