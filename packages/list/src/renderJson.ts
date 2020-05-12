import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import { PackageNode } from 'dependencies-hierarchy'
import R = require('ramda')
import getPkgInfo from './getPkgInfo'
import { PackageDependencyHierarchy } from './types'

const sortPackages = R.sortBy(R.path(['pkg', 'alias']) as (pkg: object) => R.Ord)

export default async function (
  pkgs: PackageDependencyHierarchy[],
  opts: {
    depth: number,
    long: boolean,
    search: boolean,
  }
) {
  const jsonArr = await Promise.all(pkgs.map(async (pkg) => {
    const jsonObj = {
      name: pkg.name,

      version: pkg.version,
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
    long: boolean,
  }
): Promise<{}> {
  const dependencies = {}
  await Promise.all(
    sortPackages(entryNodes).map(async (node) => {
      const subDependencies = await toJsonResult(node.dependencies || [], opts)
      const dep = opts.long ? await getPkgInfo(node) : { alias: node.alias, from: node.name, version: node.version, resolved: node.resolved }
      if (Object.keys(subDependencies).length) {
        dep['dependencies'] = subDependencies
      }
      if (!dep.resolved) {
        delete dep.resolved
      }
      const alias = dep.alias
      delete dep.alias
      dependencies[alias] = dep
    })
  )
  return dependencies
}
