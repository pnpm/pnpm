import logger from '@pnpm/logger'
import {PackageFilesResponse} from '@pnpm/package-requester'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import {Resolution} from '@pnpm/resolver-base'
import {PackageManifest, PackageScripts} from '@pnpm/types'
import {Dependencies} from '@pnpm/types'
import {oneLine} from 'common-tags'
import crypto = require('crypto')
import path = require('path')
import R = require('ramda')
import semver = require('semver')
import {PkgGraphNode, PkgGraphNodeByNodeId} from '../api/install'
import {
  createNodeId,
  ROOT_NODE_ID,
  splitNodeId,
} from '../nodeIdUtils'
import {Pkg} from '../resolveDependencies'

export interface DepGraphNode {
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
  isBuilt?: boolean,
  requiresBuild: boolean,
}

export interface DepGraphNodesByDepPath {
  [depPath: string]: DepGraphNode
}

export default function (
  pkgGraph: PkgGraphNodeByNodeId,
  rootNodeIdsByAlias: {[alias: string]: string},
  // only the top dependencies that were already installed
  // to avoid warnings about unresolved peer dependencies
  topParents: Array<{name: string, version: string}>,
  independentLeaves: boolean,
  nodeModules: string,
): {
  depGraph: DepGraphNodesByDepPath,
  rootAbsolutePathsByAlias: {[alias: string]: string},
} {
  const pkgsByName = Object.assign(
    R.fromPairs(
      topParents.map((parent: {name: string, version: string}): R.KeyValuePair<string, ParentRef> => [
        parent.name,
        {
          depth: 0,
          version: parent.version,
        },
      ]),
    ),
    toPkgByName(R.keys(rootNodeIdsByAlias).map((alias) => ({alias, nodeId: rootNodeIdsByAlias[alias], node: pkgGraph[rootNodeIdsByAlias[alias]]}))),
  )

  const absolutePathsByNodeId = {}
  const depGraph: DepGraphNodesByDepPath = {}
  resolvePeersOfChildren(rootNodeIdsByAlias, pkgsByName, {
    absolutePathsByNodeId,
    depGraph,
    independentLeaves,
    nodeModules,
    pkgGraph,
    purePkgs: new Set(),
  })

  R.values(depGraph).forEach((node) => {
    node.children = R.keys(node.children).reduce((acc, alias) => {
      acc[alias] = absolutePathsByNodeId[node.children[alias]]
      return acc
    }, {})
  })
  return {
    depGraph,
    rootAbsolutePathsByAlias: R.keys(rootNodeIdsByAlias).reduce((rootAbsolutePathsByAlias, alias) => {
      rootAbsolutePathsByAlias[alias] = absolutePathsByNodeId[rootNodeIdsByAlias[alias]]
      return rootAbsolutePathsByAlias
    }, {}),
  }
}

function resolvePeersOfNode (
  nodeId: string,
  parentParentPkgs: ParentRefs,
  ctx: {
    pkgGraph: PkgGraphNodeByNodeId,
    absolutePathsByNodeId: {[nodeId: string]: string},
    depGraph: DepGraphNodesByDepPath,
    independentLeaves: boolean,
    nodeModules: string,
    purePkgs: Set<string>, // pure packages are those that don't rely on externally resolved peers
  },
): {[alias: string]: string} {
  const node = ctx.pkgGraph[nodeId]
  if (ctx.purePkgs.has(node.pkg.id) && ctx.depGraph[node.pkg.id].depth <= node.depth) {
    ctx.absolutePathsByNodeId[nodeId] = node.pkg.id
    return {}
  }

  const children = typeof node.children === 'function' ? node.children() : node.children
  const parentPkgs = R.isEmpty(children)
    ? parentParentPkgs
    : {
        ...parentParentPkgs,
        ...toPkgByName(R.keys(children).map((alias) => ({alias, nodeId: children[alias], node: ctx.pkgGraph[children[alias]]}))),
      }
  const unknownResolvedPeersOfChildren = resolvePeersOfChildren(children, parentPkgs, ctx, nodeId)

  const resolvedPeers = R.isEmpty(node.pkg.peerDependencies)
    ? {}
    : resolvePeers(nodeId, node, parentPkgs, ctx.pkgGraph)

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
      R.keys(allResolvedPeers).map((alias) => ({
        name: alias,
        version: ctx.pkgGraph[allResolvedPeers[alias]].pkg.version,
      })))
    modules = path.join(localLocation, peersFolder, 'node_modules')
    absolutePath = `${node.pkg.id}/${peersFolder}`
  }

  ctx.absolutePathsByNodeId[nodeId] = absolutePath
  if (!ctx.depGraph[absolutePath] || ctx.depGraph[absolutePath].depth > node.depth) {
    const independent = ctx.independentLeaves && R.isEmpty(node.children) && R.isEmpty(node.pkg.peerDependencies)
    const centralLocation = node.pkg.engineCache || path.join(node.pkg.path, 'node_modules', node.pkg.name)
    const peripheralLocation = !independent
      ? path.join(modules, node.pkg.name)
      : centralLocation
    ctx.depGraph[absolutePath] = {
      absolutePath,
      additionalInfo: node.pkg.additionalInfo,
      centralLocation,
      children: Object.assign(children, resolvedPeers),
      depth: node.depth,
      dev: node.pkg.dev,
      fetchingFiles: node.pkg.fetchingFiles,
      hasBundledDependencies: node.pkg.hasBundledDependencies,
      id: node.pkg.id,
      independent,
      installable: node.installable,
      isBuilt: !!node.pkg.engineCache,
      modules,
      name: node.pkg.name,
      optional: node.pkg.optional,
      optionalDependencies: node.pkg.optionalDependencies,
      peripheralLocation,
      prod: node.pkg.prod,
      requiresBuild: node.pkg.requiresBuild,
      resolution: node.pkg.resolution,
      version: node.pkg.version,
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
    absolutePathsByNodeId: {[nodeId: string]: string},
    independentLeaves: boolean,
    nodeModules: string,
    purePkgs: Set<string>,
    depGraph: DepGraphNodesByDepPath,
    pkgGraph: PkgGraphNodeByNodeId,
  },
  exceptNodeId?: string,
): {[alias: string]: string} {
  const allResolvedPeers: {[alias: string]: string} = {}

  for (const childNodeId of R.values(children)) {
    Object.assign(allResolvedPeers, resolvePeersOfNode(childNodeId, parentPkgs, ctx))
  }

  const unknownResolvedPeersOfChildren = R.keys(allResolvedPeers)
    .filter((alias) => !children[alias] && allResolvedPeers[alias] !== exceptNodeId)
    .reduce((acc, peer) => {
      acc[peer] = allResolvedPeers[peer]
      return acc
    }, {})

  return unknownResolvedPeersOfChildren
}

function resolvePeers (
  nodeId: string,
  node: PkgGraphNode,
  parentPkgs: ParentRefs,
  pkgGraph: PkgGraphNodeByNodeId,
): {
  [alias: string]: string,
} {
  const resolvedPeers: {[alias: string]: string} = {}
  for (const peerName in node.pkg.peerDependencies) { // tslint:disable-line:forin
    const peerVersionRange = node.pkg.peerDependencies[peerName]

    const resolved = parentPkgs[peerName]

    if (!resolved || resolved.nodeId && !pkgGraph[resolved.nodeId].installable) {
      const friendlyPath = nodeIdToFriendlyPath(nodeId, pkgGraph)
      logger.warn(oneLine`
        ${friendlyPath ? `${friendlyPath}: ` : ''}${packageFriendlyId(node.pkg)}
        requires a peer of ${peerName}@${peerVersionRange} but none was installed.`,
      )
      continue
    }

    if (!semver.satisfies(resolved.version, peerVersionRange)) {
      const friendlyPath = nodeIdToFriendlyPath(nodeId, pkgGraph)
      logger.warn(oneLine`
        ${friendlyPath ? `${friendlyPath}: ` : ''}${packageFriendlyId(node.pkg)}
        requires a peer of ${peerName}@${peerVersionRange} but version ${resolved.version} was installed.`,
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

function nodeIdToFriendlyPath (nodeId: string, pkgGraph: PkgGraphNodeByNodeId) {
  const parts = splitNodeId(nodeId).slice(2, -2)
  return R.tail(R.scan((prevNodeId, pkgId) => createNodeId(prevNodeId, pkgId), ROOT_NODE_ID, parts))
    .map((nid) => pkgGraph[nid].pkg.name)
    .join(' > ')
}

interface ParentRefs {
  [name: string]: ParentRef
}

interface ParentRef {
  version: string,
  depth: number,
  // this is null only for already installed top dependencies
  nodeId?: string,
}

function toPkgByName (nodes: Array<{alias: string, nodeId: string, node: PkgGraphNode}>): ParentRefs {
  const pkgsByName: ParentRefs = {}
  for (const node of nodes) {
    pkgsByName[node.alias] = {
      depth: node.node.depth,
      nodeId: node.nodeId,
      version: node.node.pkg.version,
    }
  }
  return pkgsByName
}

function createPeersFolderName (peers: Array<{name: string, version: string}>) {
  const folderName = peers.map((peer) => `${peer.name.replace('/', '!')}@${peer.version}`).sort().join('+')

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
