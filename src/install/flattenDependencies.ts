import path = require('path')
import {InstalledPackage} from './installMultiple'
import mkdirp from '../fs/mkdirp'
import hardlinkDir from '../fs/hardlinkDir'
import {Graph} from '../fs/graphController'

export default function flattenDependencies (id: string, store: string, pkgs: InstalledPackage[], graph: Graph) {
  const newPkgs = getNewPkgs(pkgs, [id])
  const todo = new Set(newPkgs.map(newPkg => newPkg.id).concat([id]))
  const tree: FlatTree = {}
  flattenPkgs(id, graph, 1, [], tree)
  return createFlatTree(id, store, id, graph, todo, tree, 1)
}

function getNewPkgs (pkgs: InstalledPackage[], keypath: string[]): InstalledPackage[] {
  return pkgs.filter(pkg => !pkg.fromCache).concat(
    pkgs
      .filter(pkg => keypath.indexOf(pkg.id) === -1)
      .reduce((newPkgs: InstalledPackage[], pkg: InstalledPackage) =>
        newPkgs.concat(getNewPkgs(pkg.dependencies, keypath.concat([pkg.id]))), []))
}

async function createFlatTree (id: string, store: string, root: string, graph: Graph, todo: Set<string>, tree: FlatTree, depth: number) {
  if (!todo.has(id)) return
  todo.delete(id)

  const modules = path.join(root, 'node_modules')
  await mkdirp(modules)
  await Promise.all(
    Object.keys(tree[id])
      .map(async function (depName) {
        if (!tree || !tree[id] || !tree[id][depName]) throw new Error('Error during creating flat tree')
        const target = path.join(store, tree[id][depName].id)
        if (tree[id][depName].depth > depth) {
          await hardlinkDir(target, path.join(modules, depName))
        }
        return createFlatTree(tree[id][depName].id, store, target, graph, todo, tree, depth + 1)
      })
  )
}

function flattenPkgs (id: string, graph: Graph, depth: number, keypath: string[], tree: FlatTree): DepResolutionAndDeps {
  if (tree[id]) return tree[id]
  if (keypath.indexOf(id) !== -1) return {}
  tree[id] = Object.keys(graph[id].dependencies || {})
    .reduce((prev: DepResolutionAndDeps, depName: string) => {
      const depId = graph[id].dependencies[depName]
      const subdeps = flattenPkgs(depId, graph, depth + 1, keypath.concat([id]), tree)
      prev[depName] = {
        id: depId,
        depth,
      }
      return Object.assign({}, subdeps, prev)
    }, {})
  return tree[id]
}

export type FlatTree = {
  [id: string]: DepResolutionAndDeps
}

export type DepResolutionAndDeps = {
  [depname: string]: {
    id: string,
    depth: number,
  },
}

type PackagesMap = {
  [id: string]: InstalledPackage
}
