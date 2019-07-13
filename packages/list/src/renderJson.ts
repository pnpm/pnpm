import { PackageNode } from 'dependencies-hierarchy'
import path = require('path')
import { toArchyTree } from './renderTree'

export default async function (
  project: {
    name?: string,
    version?: string,
    path: string,
  },
  tree: PackageNode[],
  opts: {
    alwaysPrintRootPackage: boolean,
    long: boolean,
  },
) {
  return JSON.stringify(await toArchyTree(tree, {
    long: opts.long,
    modules: path.join(project.path, 'node_modules'),
  }), null, 2)
}
