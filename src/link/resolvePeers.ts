import {
  Resolution,
  PackageContentInfo,
  pkgIdToFilename,
} from 'package-store'
import {Dependencies, Package} from '../types'
import R = require('ramda')
import semver = require('semver')
import logger from 'pnpm-logger'
import path = require('path')
import {InstalledPackage} from '../install/installMultiple'
import {TreeNode, TreeNodeMap} from '../api/install'

export type DependencyTreeNode = {
  name: string,
  // at this point the version is really needed only for logging
  version: string,
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
  absolutePath: string,
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
  independentLeaves: boolean,
  nodeModules: string
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
  resolvePeersOfChildren(rootNodeIds, pkgsByName, {
    tree,
    nodeIdToResolvedId,
    resolvedTree,
    independentLeaves,
    nodeModules,
  })

  R.values(resolvedTree).forEach(node => {
    node.children = node.children.map(child => nodeIdToResolvedId[child])
  })
  return resolvedTree
}

function resolvePeersOfNode (
  nodeId: string,
  parentPkgs: ParentRefs,
  ctx: {
    tree: TreeNodeMap,
    nodeIdToResolvedId: {[nodeId: string]: string},
    resolvedTree: DependencyTreeNodeMap,
    independentLeaves: boolean,
    nodeModules: string,
  }
): string[] {
  const node = ctx.tree[nodeId]

  const unknownResolvedPeersOfChildren = resolvePeersOfChildren(node.children, parentPkgs, ctx)

  const resolvedPeers = R.isEmpty(node.pkg.peerDependencies)
    ? []
    : resolvePeers(node, Object.assign({}, parentPkgs,
      toPkgByName(R.props<TreeNode>(node.children, ctx.tree))
    ), ctx.tree)

  const allResolvedPeers = R.uniq(
    unknownResolvedPeersOfChildren
      .filter(resolvedPeerNodeId => resolvedPeerNodeId !== nodeId).concat(resolvedPeers))

  let modules: string
  let absolutePath: string
  const localLocation = path.join(ctx.nodeModules, `.${pkgIdToFilename(node.pkg.id)}`)
  if (R.isEmpty(allResolvedPeers)) {
    modules = path.join(localLocation, 'node_modules')
    absolutePath = node.pkg.id
  } else {
    const peersFolder = createPeersFolderName(R.props<TreeNode>(allResolvedPeers, ctx.tree).map(node => node.pkg))
    modules = path.join(localLocation, peersFolder, 'node_modules')
    absolutePath = `${node.pkg.id}/${peersFolder}`
  }

  ctx.nodeIdToResolvedId[nodeId] = absolutePath
  if (!ctx.resolvedTree[absolutePath] || ctx.resolvedTree[absolutePath].depth > node.depth) {
    const independent = ctx.independentLeaves && !node.children.length && R.isEmpty(node.pkg.peerDependencies)
    const pathToUnpacked = path.join(node.pkg.path, 'node_modules', node.pkg.name)
    const hardlinkedLocation = !independent
      ? path.join(modules, node.pkg.name)
      : pathToUnpacked
    ctx.resolvedTree[absolutePath] = {
      name: node.pkg.name,
      version: node.pkg.version,
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
      absolutePath,
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
  ctx: {
    tree: {[nodeId: string]: TreeNode},
    nodeIdToResolvedId: {[nodeId: string]: string},
    resolvedTree: DependencyTreeNodeMap,
    independentLeaves: boolean,
    nodeModules: string,
  }
): string[] {
  const unknownResolvedPeersOfChildren: string[] = []
  const parentPkgs = Object.assign({}, parentParentPkgs,
    toPkgByName(R.props<TreeNode>(children, ctx.tree))
  )

  for (const child of children) {
    const allResolvedPeers = resolvePeersOfNode(child, parentPkgs, ctx)

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
  const resolvedPeers: string[] = []
  for (const peerName in node.pkg.peerDependencies) {
    const peerVersionRange = node.pkg.peerDependencies[peerName]

    const resolved = parentPkgs[peerName]

    if (!resolved || resolved.nodeId && !tree[resolved.nodeId].installable) {
      logger.warn(`${node.pkg.id} requires a peer of ${peerName}@${peerVersionRange} but none was installed.`)
      continue
    }

    if (!semver.satisfies(resolved.version, peerVersionRange)) {
      logger.warn(`${node.pkg.id} requires a peer of ${peerName}@${peerVersionRange} but version ${resolved.version} was installed.`)
    }

    if (resolved.depth === 0 || resolved.depth === node.depth + 1) {
      // if the resolved package is a top dependency
      // or the peer dependency is resolved from a regular dependency of the package
      // then there is no need to link it in
      continue
    }

    if (resolved && resolved.nodeId) resolvedPeers.push(resolved.nodeId)
  }
  return resolvedPeers
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

function toPkgByName (nodes: TreeNode[]): ParentRefs {
  const pkgsByName: ParentRefs = {}
  for (const node of nodes) {
    pkgsByName[node.pkg.name] = {
      version: node.pkg.version,
      nodeId: node.nodeId,
      depth: node.depth,
    }
  }
  return pkgsByName
}

function createPeersFolderName(peers: InstalledPackage[]) {
  return peers.map(peer => `${peer.name.replace('/', '!')}@${peer.version}`).sort().join('+')
}
