import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import { DependenciesHierarchy, PackageNode } from 'dependencies-hierarchy'
import R = require('ramda')
import getPkgInfo from './getPkgInfo'

const sortPackages = R.sortBy(R.path(['pkg', 'alias']) as (pkg: object) => R.Ord)

export default async function (
  project: {
    name?: string,
    version?: string,
    path: string,
  },
  tree: DependenciesHierarchy,
  opts: {
    depth: number,
    long: boolean,
    search: boolean,
  },
) {
  const jsonObj = {
    name: project.name,

    version: project.version,
  }
  for (const dependenciesField of [...DEPENDENCIES_FIELDS.sort(), 'unsavedDependencies']) {
    if (tree[dependenciesField] && tree[dependenciesField].length) {
      jsonObj[dependenciesField] = await toJsonResult(tree[dependenciesField], { long: opts.long })
    }
  }
  return JSON.stringify(jsonObj, null, 2)
}

export async function toJsonResult (
  entryNodes: PackageNode[],
  opts: {
    long: boolean,
  },
): Promise<{}> {
  const dependencies = {}
  await Promise.all(
    sortPackages(entryNodes).map(async (node) => {
      const subDependencies = await toJsonResult(node.dependencies || [], opts)
      const dep = opts.long ? await getPkgInfo(node.pkg) : { alias: node.pkg.alias, from: node.pkg.name, version: node.pkg.version, resolved: node.pkg.resolved }
      if (Object.keys(subDependencies).length) {
        dep['dependencies'] = subDependencies
      }
      if (!dep.resolved) {
        delete dep.resolved
      }
      const alias = dep.alias
      delete dep.alias
      dependencies[alias] = dep
    }),
  )
  return dependencies
}
