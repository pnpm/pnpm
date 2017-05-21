import {LinkedPackagesMap, LinkedPackage} from '.'
import {Resolution} from '../resolve'
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
  fetchingFiles: Promise<boolean>,
  resolution: Resolution,
  hardlinkedLocation: string,
  children: string[],
  depth: number,
  resolvedId: string,
  dev: boolean,
  optional: boolean,
  id: string,
}

export type DependencyTreeNodeMap = {
  // a node ID is the join of the package's keypath with a colon
  // E.g., a subdeps node ID which parent is `foo` will be
  // registry.npmjs.org/foo/1.0.0:registry.npmjs.org/bar/1.0.0
  [nodeId: string]: DependencyTreeNode
}

export default function (
  pkgsMap: LinkedPackagesMap,
  topPkgIds: string[],
  // only the top dependencies that were already installed
  // to avoid warnings about unresolved peer dependencies
  topParents: {name: string, version: string}[]
): DependencyTreeNodeMap {
  const tree = createTree(pkgsMap, topPkgIds)

  const pkgsByName = R.fromPairs(
    topParents.map((parent: {name: string, version: string}): R.KeyValuePair<string, ParentRef> => [
      parent.name,
      {
        version: parent.version,
        depth: 0
      }
    ])
  )

  const nodeIdToResolvedId = {}
  const resolvedTree = resolvePeersOfChildren(tree.rootNodeIds, pkgsByName, tree.nodes, nodeIdToResolvedId).resolvedTree

  R.values(resolvedTree).forEach(node => {
    node.children = node.children.map(child => nodeIdToResolvedId[child])
  })
  return resolvedTree
}

type Tree = {
  nodes: {[nodeId: string]: TreeNode},
  rootNodeIds: string[],
}

type TreeNode = {
  nodeId: string,
  children: string[], // Node IDs of children
  pkg: LinkedPackage,
  depth: number,
}

function createTree (
  pkgsMap: LinkedPackagesMap,
  pkgIds: string[],
  depth?: number,
  parentNodeId?: string
): Tree {
  return R.props(pkgIds, pkgsMap)
    .reduce((acc: Tree, pkg: LinkedPackage) => {
      const node = createTreeNode(pkgsMap, pkg, depth || 0, parentNodeId)
      return {
        rootNodeIds: R.append(node.nodeId, acc.rootNodeIds),
        nodes: Object.assign(acc.nodes, node.childNodes)
      }
    }, {rootNodeIds: [], nodes: {}})
}

function createTreeNode (
  pkgsMap: LinkedPackagesMap,
  pkg: LinkedPackage,
  depth: number,
  parentNodeId?: string
): {
  nodeId: string,
  childNodes: {[nodeId: string]: TreeNode},
} {
  const nonCircularDeps = parentNodeId
    ? getNonCircularDependencies(parentNodeId, pkg.id, pkg.dependencies)
    : pkg.dependencies
  const nodeId = parentNodeId
    ? relationCode(parentNodeId, pkg.id)
    : pkg.id
  const tree = createTree(pkgsMap, nonCircularDeps, depth + 1, nodeId)
  return {
    nodeId,
    childNodes: Object.assign(tree.nodes, R.objOf(nodeId, {
      pkg,
      nodeId,
      children: tree.rootNodeIds,
      depth,
    }))
  }
}

function getNonCircularDependencies (
  parentNodeId: string,
  parentId: string,
  dependencyIds: string[]
) {
  return dependencyIds.filter(depId => {
    const relation = relationCode(parentId, depId)
    return parentNodeId.indexOf(relation) === -1
  })
}

function relationCode (parentId: string, dependencyId: string) {
  // using colon as it will never be used inside a package ID
  return `${parentId}:${dependencyId}`
}

function resolvePeersOfNode (
  nodeId: string,
  parentPkgs: ParentRefs,
  tree: {[nodeId: string]: TreeNode},
  nodeIdToResolvedId: {[nodeId: string]: string}
): {
  resolvedTree: DependencyTreeNodeMap,
  allResolvedPeers: string[],
} {
  const node = tree[nodeId]

  const result = resolvePeersOfChildren(node.children, parentPkgs, tree, nodeIdToResolvedId)

  const resolvedPeers = R.isEmpty(node.pkg.peerDependencies)
    ? []
    : resolvePeers(node, Object.assign({}, parentPkgs,
      toPkgByName(R.props<TreeNode>(node.children, tree))
    ))

  const allResolvedPeers = R.uniq(
    result.unknownResolvedPeersOfChildren
      .filter(resolvedPeerNodeId => resolvedPeerNodeId !== nodeId).concat(resolvedPeers))

  let modules: string
  let resolvedId: string
  if (R.isEmpty(allResolvedPeers)) {
    modules = path.join(node.pkg.localLocation, 'node_modules')
    resolvedId = node.pkg.id
  } else {
    const peersFolder = createPeersFolderName(R.props<TreeNode>(allResolvedPeers, tree).map(node => node.pkg))
    modules = path.join(node.pkg.localLocation, peersFolder, 'node_modules')
    resolvedId = `${node.pkg.id}/${peersFolder}`
  }

  nodeIdToResolvedId[nodeId] = resolvedId
  if (!result.resolvedTree[resolvedId] || result.resolvedTree[resolvedId].depth > node.depth) {
    const hardlinkedLocation = path.join(modules, node.pkg.name)
    result.resolvedTree[resolvedId] = {
      name: node.pkg.name,
      hasBundledDependencies: node.pkg.hasBundledDependencies,
      fetchingFiles: node.pkg.fetchingFiles,
      resolution: node.pkg.resolution,
      path: node.pkg.path,
      modules,
      hardlinkedLocation,
      children: R.union(node.children, resolvedPeers),
      depth: node.depth,
      resolvedId,
      dev: node.pkg.dev,
      optional: node.pkg.optional,
      id: node.pkg.id,
    }
  }
  return {
    allResolvedPeers,
    resolvedTree: result.resolvedTree,
  }
}

function resolvePeersOfChildren (
  children: string[],
  parentParentPkgs: ParentRefs,
  tree: {[nodeId: string]: TreeNode},
  nodeIdToResolvedId: {[nodeId: string]: string}
): {
  resolvedTree: DependencyTreeNodeMap,
  unknownResolvedPeersOfChildren: string[],
} {
  const trees: DependencyTreeNodeMap[] = []
  const unknownResolvedPeersOfChildren: string[] = []
  const parentPkgs = Object.assign({}, parentParentPkgs,
    toPkgByName(R.props<TreeNode>(children, tree))
  )

  for (const child of children) {
    const result = resolvePeersOfNode(child, parentPkgs, tree, nodeIdToResolvedId)
    trees.push(result.resolvedTree)

    const unknownResolvedPeersOfChild = result.allResolvedPeers
      .filter((resolvedPeerNodeId: string) => children.indexOf(resolvedPeerNodeId) === -1)

    unknownResolvedPeersOfChildren.push.apply(unknownResolvedPeersOfChildren, unknownResolvedPeersOfChild)
  }

  const resolvedTree: DependencyTreeNodeMap = trees.length ? Object.assign.apply(null, trees) : {}
  return {
    resolvedTree,
    unknownResolvedPeersOfChildren,
  }
}

function resolvePeers (
  node: TreeNode,
  parentPkgs: ParentRefs
): string[] {
  return R.toPairs(node.pkg.peerDependencies)
    .map(R.apply((peerName: string, peerVersionRange: string) => {
      const resolved = parentPkgs[peerName]

      if (!resolved) {
        logger.warn(`${node.pkg.id} requires a peer of ${peerName}@${peerVersionRange} but none was installed.`)
        return null
      }

      if (!semver.satisfies(resolved.version, peerVersionRange)) {
        logger.warn(`${node.pkg.id} requires a peer of ${peerName}@${peerVersionRange} but version ${resolved.version} was installed.`)
      }

      if (resolved.depth === 0 || resolved.depth === node.depth + 1) {
        // if the resolved package is a top dependency
        // or the peer dependency is resolved from a regular dependency of the package
        // then there is no need to link it in
        return null
      }

      return resolved && resolved.nodeId
    }))
    .filter(Boolean) as string[]
}

type ParentRefs = {
  [name: string]: ParentRef
}

type ParentRef = {
  version: string,
  depth: number,
  // this is null only for already installed top dependencies
  nodeId?: string,
}

function toPkgByName(pkgs: TreeNode[]): ParentRefs {
  const toNameAndPkg = R.map((node: TreeNode): R.KeyValuePair<string, ParentRef> => [
    node.pkg.name,
    {
      version: node.pkg.version,
      nodeId: node.nodeId,
      depth: node.depth,
    }
  ])
  return R.fromPairs(toNameAndPkg(pkgs))
}

function createPeersFolderName(peers: LinkedPackage[]) {
  return peers.map(peer => `${peer.name.replace('/', '!')}@${peer.version}`).sort().join('+')
}
