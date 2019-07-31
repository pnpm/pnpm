import { PackageNode } from 'dependencies-hierarchy'
import getPkgInfo from './getPkgInfo'

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
  return JSON.stringify({
    ...project,
    dependencies: await toJsonResult(tree, { long: opts.long }),
  }, null, 2)
}

export async function toJsonResult (
  entryNodes: PackageNode[],
  opts: {
    long: boolean,
  },
): Promise<Array<{}>> {
  return Promise.all(
    entryNodes.map(async (node) => {
      const dependencies = await toJsonResult(node.dependencies || [], opts)
      const result = opts.long ? await getPkgInfo(node.pkg) : node.pkg
      if (dependencies.length) {
        result['dependencies'] = dependencies
      }
      return result
    }),
  )
}
