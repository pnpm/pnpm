import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import { PackageNode } from 'dependencies-hierarchy'
import sortBy from 'ramda/src/sortBy'
import path from 'ramda/src/path'
import { Ord } from 'ramda'
import getPkgInfo from './getPkgInfo'
import { PackageDependencyHierarchy } from './types'

const sortPackages = sortBy(path(['pkg', 'alias']) as (pkg: object) => Ord)

export default async function (
  pkgs: PackageDependencyHierarchy[],
  opts: {
    depth: number
    long: boolean
    search: boolean
  }
) {
  const jsonArr = await Promise.all(pkgs.map(async (pkg) => {
    const jsonObj = {
      name: pkg.name,
      version: pkg.version,
      path: pkg.path,
    }
    for (const dependenciesField of [...DEPENDENCIES_FIELDS.sort(), 'unsavedDependencies']) {
      if (pkg[dependenciesField]?.length) {
        jsonObj[dependenciesField] = await toJsonResult(pkg[dependenciesField], { long: opts.long })
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
): Promise<{}> {
  const dependencies = {}
  await Promise.all(
    sortPackages(entryNodes).map(async (node) => {
      const subDependencies = await toJsonResult(node.dependencies ?? [], opts)
      const dep = opts.long
        ? await getPkgInfo(node)
        : {
          alias: node.alias as string | undefined,
          from: node.name,
          version: node.version,

          resolved: node.resolved,
        }
      if (Object.keys(subDependencies).length > 0) {
        dep['dependencies'] = subDependencies
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
