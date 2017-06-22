import {Resolution} from '../resolve'
import {Dependencies, Package} from '../types'
import R = require('ramda')
import semver = require('semver')
import logger from 'pnpm-logger'
import path = require('path')
import {InstalledPackage} from '../install/installMultiple'
import {TreeNode, TreeNodeMap, PackageContentInfo} from '../api/install'

export type DependencyTreeNode = {
  name: string,
  hasBundledDependencies: boolean,
  path: string,
  modules: string,
  fetchingFiles: Promise<PackageContentInfo>,
  resolution: Resolution,
  hardlinkedLocation: string,
  children: string[],
  // an independent package is a package that
  // has neither regular nor peer dependencies
  independent: boolean,
  optionalDependencies: Set<string>,
  depth: number,
  resolvedId: string,
  dev: boolean,
  optional: boolean,
  id: string,
  installable: boolean,
}

export type DependencyTreeNodeMap = {
  // a node ID is the join of the package's keypath with a colon
  // E.g., a subdeps node ID which parent is `foo` will be
  // registry.npmjs.org/foo/1.0.0:registry.npmjs.org/bar/1.0.0
  [nodeId: string]: DependencyTreeNode
}

export default function (
  tree: TreeNodeMap,
  rootNodeIds: string[],
  topPkgIds: string[],
  // only the top dependencies that were already installed
  // to avoid warnings about unresolved peer dependencies
  topParents: {name: string, version: string}[],
  independentLeaves: boolean
): DependencyTreeNodeMap {
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
  const resolvedTree: DependencyTreeNodeMap = {}
  resolvePeersOfChildren(rootNodeIds, pkgsByName, tree, nodeIdToResolvedId, resolvedTree, independentLeaves)

  R.values(resolvedTree).forEach(node => {
    node.children = node.children.map(child => nodeIdToResolvedId[child])
  })
  return resolvedTree
}

function resolvePeersOfNode (
  nodeId: string,
  parentPkgs: ParentRefs,
  tree: TreeNodeMap,
  nodeIdToResolvedId: {[nodeId: string]: string},
  resolvedTree: DependencyTreeNodeMap,
  independentLeaves: boolean
): string[] {
  const node = tree[nodeId]

  const unknownResolvedPeersOfChildren = resolvePeersOfChildren(node.children, parentPkgs, tree, nodeIdToResolvedId, resolvedTree, independentLeaves)

  const resolvedPeers = R.isEmpty(node.pkg.peerDependencies)
    ? []
    : resolvePeers(node, Object.assign({}, parentPkgs,
      toPkgByName(R.props<TreeNode>(node.children, tree))
    ), tree)

  const allResolvedPeers = R.uniq(
    unknownResolvedPeersOfChildren
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
  if (!resolvedTree[resolvedId] || resolvedTree[resolvedId].depth > node.depth) {
    const independent = independentLeaves && !node.children.length && R.isEmpty(node.pkg.peerDependencies)
    const pathToUnpacked = path.join(node.pkg.path, 'node_modules', node.pkg.name)
    const hardlinkedLocation = !independent
      ? path.join(modules, node.pkg.name)
      : pathToUnpacked
    resolvedTree[resolvedId] = {
      name: node.pkg.name,
      hasBundledDependencies: node.pkg.hasBundledDependencies,
      fetchingFiles: node.pkg.fetchingFiles,
      resolution: node.pkg.resolution,
      path: pathToUnpacked,
      modules,
      hardlinkedLocation,
      independent,
      optionalDependencies: node.pkg.optionalDependencies,
      children: R.union(node.children, resolvedPeers),
      depth: node.depth,
      resolvedId,
      dev: node.pkg.dev,
      optional: node.pkg.optional,
      id: node.pkg.id,
      installable: node.installable,
    }
  }
  return allResolvedPeers
}

function resolvePeersOfChildren (
  children: string[],
  parentParentPkgs: ParentRefs,
  tree: {[nodeId: string]: TreeNode},
  nodeIdToResolvedId: {[nodeId: string]: string},
  resolvedTree: DependencyTreeNodeMap,
  independentLeaves: boolean
): string[] {
  const trees: DependencyTreeNodeMap[] = []
  const unknownResolvedPeersOfChildren: string[] = []
  const parentPkgs = Object.assign({}, parentParentPkgs,
    toPkgByName(R.props<TreeNode>(children, tree))
  )

  for (const child of children) {
    const allResolvedPeers = resolvePeersOfNode(child, parentPkgs, tree, nodeIdToResolvedId, resolvedTree, independentLeaves)

    const unknownResolvedPeersOfChild = allResolvedPeers
      .filter((resolvedPeerNodeId: string) => children.indexOf(resolvedPeerNodeId) === -1)

    unknownResolvedPeersOfChildren.push.apply(unknownResolvedPeersOfChildren, unknownResolvedPeersOfChild)
  }

  return unknownResolvedPeersOfChildren
}

function resolvePeers (
  node: TreeNode,
  parentPkgs: ParentRefs,
  tree: TreeNodeMap
): string[] {
  return R.toPairs(node.pkg.peerDependencies)
    .map(R.apply((peerName: string, peerVersionRange: string) => {
      const resolved = parentPkgs[peerName]

      if (!resolved || resolved.nodeId && !tree[resolved.nodeId].installable) {
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

function createPeersFolderName(peers: InstalledPackage[]) {
  return peers.map(peer => `${peer.name.replace('/', '!')}@${peer.version}`).sort().join('+')
}
