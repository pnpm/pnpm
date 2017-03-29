import {LinkedPackagesMap, LinkedPackage} from '.'
import {Dependencies, Package} from '../types'
import R = require('ramda')
import semver = require('semver')
import logger from 'pnpm-logger'
import path = require('path')

export type DependencyTreeNode = {
  name: string,
  hasBundledDependencies: boolean,
  path: string,
  modules: string,
  peerModules?: string,
  fetchingFiles: Promise<boolean>,
  hardlinkedLocation: string,
  children: string[],
  resolvedPeers: string[],
  depth: number,
  id: string,
}

export type DependencyTreeNodeMap = {
  [nodeId: string]: DependencyTreeNode
}

export default function (
  pkgsMap: LinkedPackagesMap,
  topPkgIds: string[]
): DependencyTreeNodeMap {
  const tree = createTree(pkgsMap, topPkgIds, [])

  const pkgsByName = toPkgByName(R.props<TreeNode>(tree.rootNodeIds, tree.nodes))
  const resolvedTreeMap = R.reduce(R.merge, {}, tree.rootNodeIds.map(rootNodeId => resolvePeersOfNode(rootNodeId, pkgsByName, tree.nodes)))
  return resolvedTreeMap
}

type Tree = {
  nodes: {[nodeId: string]: TreeNode},
  rootNodeIds: string[],
}

type TreeNode = {
  nodeId: string,
  children: string[],
  pkg: LinkedPackage,
  depth: number,
}

function createTree (
  pkgsMap: LinkedPackagesMap,
  pkgIds: string[],
  keypath: string[]
): Tree {
  return R.props(pkgIds, pkgsMap)
    .reduce((acc: Tree, pkg: LinkedPackage) => {
      const node = createTreeNode(pkgsMap, pkg, keypath)
      return {
        rootNodeIds: R.append(node.nodeId, acc.rootNodeIds),
        nodes: R.merge(acc.nodes, node.childNodes)
      }
    }, {rootNodeIds: [], nodes: {}})
}

function createTreeNode (
  pkgsMap: LinkedPackagesMap,
  pkg: LinkedPackage,
  keypath: string[]
): {
  nodeId: string,
  childNodes: {[nodeId: string]: TreeNode},
} {
  const nonCircularDeps = getNonCircularDependencies(pkg.id, pkg.dependencies, keypath)
  const newKeypath = R.append(pkg.id, keypath)
  const nodeId = newKeypath.join('/')
  const tree = createTree(pkgsMap, nonCircularDeps, newKeypath)
  return {
    nodeId,
    childNodes: R.merge(R.objOf(nodeId, {
      pkg,
      nodeId,
      children: tree.rootNodeIds,
      depth: keypath.length,
    }), tree.nodes)
  }
}

function getNonCircularDependencies (
  parentId: string,
  dependencyIds: string[],
  keypath: string[]
) {
  const relations = R.aperture(2, keypath)
  const isCircular = R.partialRight(R.contains, [relations])
  return dependencyIds.filter(depId => !isCircular([parentId, depId]))
}

function resolvePeersOfNode (
  nodeId: string,
  parentPkgs: {[name: string]: TreeNode},
  tree: {[nodeId: string]: TreeNode}
): DependencyTreeNodeMap {
  const node = tree[nodeId]
  const newParentPkgs = Object.assign({}, parentPkgs,
    {[node.pkg.name]: node},
    toPkgByName(R.props<TreeNode>(node.children, tree))
  )

  const resolvedPeers = resolvePeers(node.pkg.peerDependencies, node.pkg.id, newParentPkgs)

  const modules = path.join(node.pkg.localLocation, 'node_modules')
  const peerModules = !R.isEmpty(node.pkg.peerDependencies)
    ? path.join(node.pkg.localLocation, createPeersFolderName(R.props<TreeNode>(resolvedPeers, tree).map(node => node.pkg)), 'node_modules')
    : undefined

  const hardlinkedLocation = path.join(peerModules || modules, node.pkg.name)

  return R.reduce(R.merge, R.objOf(nodeId, {
    name: node.pkg.name,
    hasBundledDependencies: node.pkg.hasBundledDependencies,
    fetchingFiles: node.pkg.fetchingFiles,
    path: node.pkg.path,
    peerModules,
    modules,
    hardlinkedLocation,
    resolvedPeers,
    children: node.children,
    depth: node.depth,
    id: node.pkg.id,
  }), node.children.map(child => resolvePeersOfNode(child, newParentPkgs, tree)))
}

function resolvePeers (
  peerDependencies: Dependencies,
  pkgId: string,
  parentPkgs: {[name: string]: TreeNode},
): string[] {
  return R.toPairs(peerDependencies)
    .map(R.apply((peerName: string, peerVersionRange: string) => {
      const resolved = parentPkgs[peerName]

      if (!resolved) {
        logger.warn(`${pkgId} requires a peer of ${peerName}@${peerVersionRange} but none was installed.`)
        return null
      }

      if (!semver.satisfies(resolved.pkg.version, peerVersionRange)) {
        logger.warn(`${pkgId} requires a peer of ${peerName}@${peerVersionRange} but version ${resolved.pkg.version} was installed.`)
      }

      return resolved && resolved.nodeId
    }))
    .filter(Boolean) as string[]
}

function toPkgByName(pkgs: TreeNode[]): {[pkgName: string]: TreeNode} {
  const toNameAndPkg = R.map((node: TreeNode): R.KeyValuePair<string, TreeNode> => [node.pkg.name, node])
  return R.fromPairs(toNameAndPkg(pkgs))
}

function createPeersFolderName(peers: LinkedPackage[]) {
  return peers.map(peer => `${peer.name.replace('/', '!')}@${peer.version}`).sort().join('+')
}
