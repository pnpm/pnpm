import { PackageNode } from 'dependencies-hierarchy'
import R = require('ramda')
import getPkgInfo from './getPkgInfo'

const sortPackages = R.sortBy(R.path(['pkg', 'alias']) as (pkg: object) => R.Ord)

export default async function (
  project: {
    name?: string,
    version?: string,
    path: string,
  },
  tree: PackageNode[],
  opts: {
    long: boolean,
  },
) {
  return JSON.stringify({
    name: project.name,

    version: project.version,

    dependencies: await toJsonResult(tree, { long: opts.long }),
  }, null, 2)
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
      const dep = opts.long ? await getPkgInfo(node.pkg) : { alias: node.pkg.alias, from: node.pkg.name, version: node.pkg.version }
      if (Object.keys(subDependencies).length) {
        dep['dependencies'] = subDependencies
      }
      const alias = dep.alias
      delete dep.alias
      dependencies[alias] = dep
    }),
  )
  return dependencies
}
