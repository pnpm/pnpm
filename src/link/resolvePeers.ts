import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import {
  Resolution,
  PackageFilesResponse,
} from '@pnpm/package-requester'
import {Dependencies} from '@pnpm/types'
import R = require('ramda')
import semver = require('semver')
import logger from '@pnpm/logger'
import {PackageManifest} from '@pnpm/types'
import path = require('path')
import {oneLine} from 'common-tags'
import crypto = require('crypto')
import {InstalledPackage} from '../install/installMultiple'
import {TreeNode, TreeNodeMap} from '../api/install'
import {
  splitNodeId,
  createNodeId,
  ROOT_NODE_ID,
} from '../nodeIdUtils'

export type DependencyTreeNode = {
  name: string,
  // at this point the version is really needed only for logging
  version: string,
  hasBundledDependencies: boolean,
  centralLocation: string,
  modules: string,
  fetchingFiles: Promise<PackageFilesResponse>,
  resolution: Resolution,
  peripheralLocation: string,
  children: {[alias: string]: string},
  // an independent package is a package that
  // has neither regular nor peer dependencies
  independent: boolean,
  optionalDependencies: Set<string>,
  depth: number,
  absolutePath: string,
  prod: boolean,
  dev: boolean,
  optional: boolean,
  id: string,
  installable: boolean,
  additionalInfo: {
    deprecated?: string,
    peerDependencies?: Dependencies,
    bundleDependencies?: string[],
    bundledDependencies?: string[],
    engines?: {
      node?: string,
      npm?: string,
    },
    cpu?: string[],
    os?: string[],
  },
}

export type DependencyTreeNodeMap = {
  // a node ID is the join of the package's keypath with a colon
  // E.g., a subdeps node ID which parent is `foo` will be
  // registry.npmjs.org/foo/1.0.0:registry.npmjs.org/bar/1.0.0
  [nodeId: string]: DependencyTreeNode
}

export default function (
  tree: TreeNodeMap,
  rootNodeIdsByAlias: {[alias: string]: string},
  // only the top dependencies that were already installed
  // to avoid warnings about unresolved peer dependencies
  topParents: {name: string, version: string}[],
  independentLeaves: boolean,
  nodeModules: string
): {
  resolvedTree: DependencyTreeNodeMap,
  rootAbsolutePathsByAlias: {[alias: string]: string},
} {
  const pkgsByName = Object.assign(
    R.fromPairs(
      topParents.map((parent: {name: string, version: string}): R.KeyValuePair<string, ParentRef> => [
        parent.name,
        {
          version: parent.version,
          depth: 0
        }
      ])
    ),
    toPkgByName(R.keys(rootNodeIdsByAlias).map(alias => ({alias, nodeId: rootNodeIdsByAlias[alias], node: tree[rootNodeIdsByAlias[alias]]})))
  )

  const absolutePathsByNodeId = {}
  const resolvedTree: DependencyTreeNodeMap = {}
  resolvePeersOfChildren(rootNodeIdsByAlias, pkgsByName, {
    tree,
    absolutePathsByNodeId,
    resolvedTree,
    independentLeaves,
    nodeModules,
    purePkgs: new Set(),
  })

  R.values(resolvedTree).forEach(node => {
    node.children = R.keys(node.children).reduce((acc, alias) => {
      acc[alias] = absolutePathsByNodeId[node.children[alias]]
      return acc
    }, {})
  })
  return {
    resolvedTree,
    rootAbsolutePathsByAlias: R.keys(rootNodeIdsByAlias).reduce((rootAbsolutePathsByAlias, alias) => {
      rootAbsolutePathsByAlias[alias] = absolutePathsByNodeId[rootNodeIdsByAlias[alias]]
      return rootAbsolutePathsByAlias
    }, {})
  }
}

function resolvePeersOfNode (
  nodeId: string,
  parentParentPkgs: ParentRefs,
  ctx: {
    tree: TreeNodeMap,
    absolutePathsByNodeId: {[nodeId: string]: string},
    resolvedTree: DependencyTreeNodeMap,
    independentLeaves: boolean,
    nodeModules: string,
    purePkgs: Set<string>, // pure packages are those that don't rely on externally resolved peers
  }
): {[alias: string]: string} {
  const node = ctx.tree[nodeId]
  if (ctx.purePkgs.has(node.pkg.id) && ctx.resolvedTree[node.pkg.id].depth <= node.depth) {
    ctx.absolutePathsByNodeId[nodeId] = node.pkg.id
    return {}
  }

  const children = typeof node.children === 'function' ? node.children() : node.children
  const parentPkgs = R.isEmpty(children)
    ? parentParentPkgs
    : {
        ...parentParentPkgs,
        ...toPkgByName(R.keys(children).map(alias => ({alias, nodeId: children[alias], node: ctx.tree[children[alias]]})))
      }
  const unknownResolvedPeersOfChildren = resolvePeersOfChildren(children, parentPkgs, ctx, nodeId)

  const resolvedPeers = R.isEmpty(node.pkg.peerDependencies)
    ? {}
    : resolvePeers(nodeId, node, parentPkgs, ctx.tree)

  const allResolvedPeers = Object.assign(unknownResolvedPeersOfChildren, resolvedPeers)

  let modules: string
  let absolutePath: string
  const localLocation = path.join(ctx.nodeModules, `.${pkgIdToFilename(node.pkg.id)}`)
  if (R.isEmpty(allResolvedPeers)) {
    modules = path.join(localLocation, 'node_modules')
    absolutePath = node.pkg.id
    if (R.isEmpty(node.pkg.peerDependencies)) {
      ctx.purePkgs.add(node.pkg.id)
    }
  } else {
    const peersFolder = createPeersFolderName(
      R.keys(allResolvedPeers).map(alias => ({
        name: alias,
        version: ctx.tree[allResolvedPeers[alias]].pkg.version,
      })))
    modules = path.join(localLocation, peersFolder, 'node_modules')
    absolutePath = `${node.pkg.id}/${peersFolder}`
  }

  ctx.absolutePathsByNodeId[nodeId] = absolutePath
  if (!ctx.resolvedTree[absolutePath] || ctx.resolvedTree[absolutePath].depth > node.depth) {
    const independent = ctx.independentLeaves && R.isEmpty(node.children) && R.isEmpty(node.pkg.peerDependencies)
    const centralLocation = path.join(node.pkg.path, 'node_modules', node.pkg.name)
    const peripheralLocation = !independent
      ? path.join(modules, node.pkg.name)
      : centralLocation
    ctx.resolvedTree[absolutePath] = {
      name: node.pkg.name,
      version: node.pkg.version,
      hasBundledDependencies: node.pkg.hasBundledDependencies,
      fetchingFiles: node.pkg.fetchingFiles,
      resolution: node.pkg.resolution,
      centralLocation,
      modules,
      peripheralLocation,
      independent,
      optionalDependencies: node.pkg.optionalDependencies,
      children: Object.assign(children, resolvedPeers),
      depth: node.depth,
      absolutePath,
      prod: node.pkg.prod,
      dev: node.pkg.dev,
      optional: node.pkg.optional,
      id: node.pkg.id,
      installable: node.installable,
      additionalInfo: node.pkg.additionalInfo,
    }
  }
  return allResolvedPeers
}

function resolvePeersOfChildren (
  children: {
    [alias: string]: string,
  },
  parentPkgs: ParentRefs,
  ctx: {
    tree: {[nodeId: string]: TreeNode},
    absolutePathsByNodeId: {[nodeId: string]: string},
    resolvedTree: DependencyTreeNodeMap,
    independentLeaves: boolean,
    nodeModules: string,
    purePkgs: Set<string>,
  },
  exceptNodeId?: string,
): {[alias: string]: string} {
  let allResolvedPeers: {[alias: string]: string} = {}

  for (const childNodeId of R.values(children)) {
    Object.assign(allResolvedPeers, resolvePeersOfNode(childNodeId, parentPkgs, ctx))
  }

  const unknownResolvedPeersOfChildren = R.keys(allResolvedPeers)
    .filter(alias => !children[alias] && allResolvedPeers[alias] !== exceptNodeId)
    .reduce((unknownResolvedPeersOfChildren, peer) => {
      unknownResolvedPeersOfChildren[peer] = allResolvedPeers[peer]
      return unknownResolvedPeersOfChildren
    }, {})

  return unknownResolvedPeersOfChildren
}

function resolvePeers (
  nodeId: string,
  node: TreeNode,
  parentPkgs: ParentRefs,
  tree: TreeNodeMap
): {
  [alias: string]: string
} {
  const resolvedPeers: {[alias: string]: string} = {}
  for (const peerName in node.pkg.peerDependencies) {
    const peerVersionRange = node.pkg.peerDependencies[peerName]

    const resolved = parentPkgs[peerName]

    if (!resolved || resolved.nodeId && !tree[resolved.nodeId].installable) {
      const friendlyPath = nodeIdToFriendlyPath(nodeId, tree)
      logger.warn(oneLine`
        ${friendlyPath ? `${friendlyPath}: ` : ''}${packageFriendlyId(node.pkg)}
        requires a peer of ${peerName}@${peerVersionRange} but none was installed.`
      )
      continue
    }

    if (!semver.satisfies(resolved.version, peerVersionRange)) {
      const friendlyPath = nodeIdToFriendlyPath(nodeId, tree)
      logger.warn(oneLine`
        ${friendlyPath ? `${friendlyPath}: ` : ''}${packageFriendlyId(node.pkg)}
        requires a peer of ${peerName}@${peerVersionRange} but version ${resolved.version} was installed.`
      )
    }

    if (resolved.depth === 0 || resolved.depth === node.depth + 1) {
      // if the resolved package is a top dependency
      // or the peer dependency is resolved from a regular dependency of the package
      // then there is no need to link it in
      continue
    }

    if (resolved && resolved.nodeId) resolvedPeers[peerName] = resolved.nodeId
  }
  return resolvedPeers
}

function packageFriendlyId (pkg: {name: string, version: string}) {
  return `${pkg.name}@${pkg.version}`
}

function nodeIdToFriendlyPath (nodeId: string, tree: TreeNodeMap) {
  const parts = splitNodeId(nodeId).slice(2, -2)
  return R.tail(R.scan((prevNodeId, pkgId) => createNodeId(prevNodeId, pkgId), ROOT_NODE_ID, parts))
    .map(nodeId => tree[nodeId].pkg.name)
    .join(' > ')
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

function toPkgByName (nodes: {alias: string, nodeId: string, node: TreeNode}[]): ParentRefs {
  const pkgsByName: ParentRefs = {}
  for (const node of nodes) {
    pkgsByName[node.alias] = {
      version: node.node.pkg.version,
      nodeId: node.nodeId,
      depth: node.node.depth,
    }
  }
  return pkgsByName
}

function createPeersFolderName(peers: {name: string, version: string}[]) {
  const folderName = peers.map(peer => `${peer.name.replace('/', '!')}@${peer.version}`).sort().join('+')

  // We don't want the folder name to get too long.
  // Otherwise, an ENAMETOOLONG error might happen.
  // see: https://github.com/pnpm/pnpm/issues/977
  //
  // A bigger limit might be fine but the md5 hash will be 32 symbols,
  // so for consistency's sake, we go with 32.
  if (folderName.length > 32) {
    return crypto.createHash('md5').update(folderName).digest('hex')
  }
  return folderName
}
