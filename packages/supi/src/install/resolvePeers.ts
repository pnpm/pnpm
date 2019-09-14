import PnpmError from '@pnpm/error'
import logger from '@pnpm/logger'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import {
  DependenciesTree,
  DependenciesTreeNode,
} from '@pnpm/resolve-dependencies'
import { Resolution } from '@pnpm/resolver-base'
import { PackageFilesResponse } from '@pnpm/store-controller-types'
import { Dependencies, DependencyManifest } from '@pnpm/types'
import {
  createNodeId,
  splitNodeId,
} from '@pnpm/utils'
import { oneLine } from 'common-tags'
import crypto = require('crypto')
import importFrom = require('import-from')
import path = require('path')
import R = require('ramda')
import semver = require('semver')

export interface DependenciesGraphNode {
  name: string,
  // at this point the version is really needed only for logging
  version: string,
  hasBin: boolean,
  hasBundledDependencies: boolean,
  centralLocation: string,
  modules: string,
  fetchingBundledManifest?: () => Promise<DependencyManifest>,
  fetchingFiles: () => Promise<PackageFilesResponse>,
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
  packageId: string,
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
  requiresBuild?: boolean,
  prepare: boolean,
  isPure: boolean,
}

export interface DependenciesGraph {
  [depPath: string]: DependenciesGraphNode
}

export default function (
  opts: {
    importers: Array<{
      directNodeIdsByAlias: {[alias: string]: string},
      // only the top dependencies that were already installed
      // to avoid warnings about unresolved peer dependencies
      topParents: Array<{name: string, version: string}>,
      prefix: string, // is only needed for logging
      id: string,
    }>,
    dependenciesTree: DependenciesTree,
    independentLeaves: boolean,
    virtualStoreDir: string,
    lockfileDirectory: string,
    strictPeerDependencies: boolean,
  },
): {
  depGraph: DependenciesGraph,
  importersDirectAbsolutePathsByAlias: {[id: string]: {[alias: string]: string}},
} {
  const depGraph: DependenciesGraph = {}
  const absolutePathsByNodeId = {}

  for (const { directNodeIdsByAlias, topParents, prefix } of opts.importers) {
    const pkgsByName = Object.assign(
      R.fromPairs(
        topParents.map(({ name, version }: {name: string, version: string}): R.KeyValuePair<string, ParentRef> => [
          name,
          {
            depth: 0,
            version,
          },
        ]),
      ),
      toPkgByName(
        Object
          .keys(directNodeIdsByAlias)
          .map((alias) => ({
            alias,
            node: opts.dependenciesTree[directNodeIdsByAlias[alias]],
            nodeId: directNodeIdsByAlias[alias],
          })),
      ),
    )

    resolvePeersOfChildren(directNodeIdsByAlias, pkgsByName, {
      absolutePathsByNodeId,
      dependenciesTree: opts.dependenciesTree,
      depGraph,
      independentLeaves: opts.independentLeaves,
      lockfileDirectory: opts.lockfileDirectory,
      prefix,
      purePkgs: new Set(),
      strictPeerDependencies: opts.strictPeerDependencies,
      virtualStoreDir: opts.virtualStoreDir,
    })
  }

  R.values(depGraph).forEach((node) => {
    node.children = R.keys(node.children).reduce((acc, alias) => {
      acc[alias] = absolutePathsByNodeId[node.children[alias]]
      return acc
    }, {})
  })

  const importersDirectAbsolutePathsByAlias: {[id: string]: {[alias: string]: string}} = {}
  for (const { directNodeIdsByAlias, id } of opts.importers) {
    importersDirectAbsolutePathsByAlias[id] = R.keys(directNodeIdsByAlias).reduce((rootAbsolutePathsByAlias, alias) => {
      rootAbsolutePathsByAlias[alias] = absolutePathsByNodeId[directNodeIdsByAlias[alias]]
      return rootAbsolutePathsByAlias
    }, {})
  }
  return {
    depGraph,
    importersDirectAbsolutePathsByAlias,
  }
}

function resolvePeersOfNode (
  nodeId: string,
  parentParentPkgs: ParentRefs,
  ctx: {
    dependenciesTree: DependenciesTree,
    absolutePathsByNodeId: {[nodeId: string]: string},
    depGraph: DependenciesGraph,
    independentLeaves: boolean,
    virtualStoreDir: string,
    purePkgs: Set<string>, // pure packages are those that don't rely on externally resolved peers
    prefix: string,
    lockfileDirectory: string,
    strictPeerDependencies: boolean,
  },
): {[alias: string]: string} {
  const node = ctx.dependenciesTree[nodeId]
  if (ctx.purePkgs.has(node.resolvedPackage.id) && ctx.depGraph[node.resolvedPackage.id].depth <= node.depth) {
    ctx.absolutePathsByNodeId[nodeId] = node.resolvedPackage.id
    return {}
  }

  const children = typeof node.children === 'function' ? node.children() : node.children
  const parentPkgs = R.isEmpty(children)
    ? parentParentPkgs
    : {
      ...parentParentPkgs,
      ...toPkgByName(Object.keys(children).map((alias) => ({ alias, nodeId: children[alias], node: ctx.dependenciesTree[children[alias]] }))),
    }
  const unknownResolvedPeersOfChildren = resolvePeersOfChildren(children, parentPkgs, ctx)

  const resolvedPeers = R.isEmpty(node.resolvedPackage.peerDependencies)
    ? {}
    : resolvePeers({
      dependenciesTree: ctx.dependenciesTree,
      node,
      nodeId,
      parentPkgs,
      prefix: ctx.prefix,
      strictPeerDependencies: ctx.strictPeerDependencies,
    })

  const allResolvedPeers = Object.assign(unknownResolvedPeersOfChildren, resolvedPeers)

  let modules: string
  let absolutePath: string
  const localLocation = path.join(ctx.virtualStoreDir, pkgIdToFilename(node.resolvedPackage.id, ctx.lockfileDirectory))
  const isPure = R.isEmpty(allResolvedPeers)
  if (isPure) {
    modules = path.join(localLocation, 'node_modules')
    absolutePath = node.resolvedPackage.id
    if (R.isEmpty(node.resolvedPackage.peerDependencies)) {
      ctx.purePkgs.add(node.resolvedPackage.id)
    }
  } else {
    const peersFolderSuffix = createPeersFolderSuffix(
      Object.keys(allResolvedPeers).map((alias) => ({
        name: alias,
        version: ctx.dependenciesTree[allResolvedPeers[alias]].resolvedPackage.version,
      })))
    modules = path.join(`${localLocation}${peersFolderSuffix}`, 'node_modules')
    absolutePath = `${node.resolvedPackage.id}${peersFolderSuffix}`
  }

  ctx.absolutePathsByNodeId[nodeId] = absolutePath
  if (!ctx.depGraph[absolutePath] || ctx.depGraph[absolutePath].depth > node.depth) {
    const independent = ctx.independentLeaves && node.resolvedPackage.independent
    const centralLocation = node.resolvedPackage.engineCache || path.join(node.resolvedPackage.path, 'node_modules', node.resolvedPackage.name)
    const peripheralLocation = !independent
      ? path.join(modules, node.resolvedPackage.name)
      : centralLocation

    const unknownPeers = Object.keys(unknownResolvedPeersOfChildren)
    if (unknownPeers.length) {
      if (!node.resolvedPackage.additionalInfo.peerDependencies) {
        node.resolvedPackage.additionalInfo.peerDependencies = {}
      }
      for (const unknownPeer of unknownPeers) {
        if (!node.resolvedPackage.additionalInfo.peerDependencies[unknownPeer]) {
          node.resolvedPackage.additionalInfo.peerDependencies[unknownPeer] = '*'
        }
      }
    }
    ctx.depGraph[absolutePath] = {
      absolutePath,
      additionalInfo: node.resolvedPackage.additionalInfo,
      centralLocation,
      children: Object.assign(children, resolvedPeers),
      depth: node.depth,
      dev: node.resolvedPackage.dev,
      fetchingBundledManifest: node.resolvedPackage.fetchingBundledManifest,
      fetchingFiles: node.resolvedPackage.fetchingFiles,
      hasBin: node.resolvedPackage.hasBin,
      hasBundledDependencies: node.resolvedPackage.hasBundledDependencies,
      independent,
      installable: node.installable,
      isBuilt: !!node.resolvedPackage.engineCache,
      isPure,
      modules,
      name: node.resolvedPackage.name,
      optional: node.resolvedPackage.optional,
      optionalDependencies: node.resolvedPackage.optionalDependencies,
      packageId: node.resolvedPackage.id,
      peripheralLocation,
      prepare: node.resolvedPackage.prepare,
      prod: node.resolvedPackage.prod,
      requiresBuild: node.resolvedPackage.requiresBuild,
      resolution: node.resolvedPackage.resolution,
      version: node.resolvedPackage.version,
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
    virtualStoreDir: string,
    purePkgs: Set<string>,
    depGraph: DependenciesGraph,
    dependenciesTree: DependenciesTree,
    prefix: string,
    lockfileDirectory: string,
    strictPeerDependencies: boolean,
  },
): {[alias: string]: string} {
  const allResolvedPeers: {[alias: string]: string} = {}

  for (const childNodeId of R.values(children)) {
    Object.assign(allResolvedPeers, resolvePeersOfNode(childNodeId, parentPkgs, ctx))
  }

  const unknownResolvedPeersOfChildren = R.keys(allResolvedPeers)
    .filter((alias) => !children[alias])
    .reduce((acc, peer) => {
      acc[peer] = allResolvedPeers[peer]
      return acc
    }, {})

  return unknownResolvedPeersOfChildren
}

function resolvePeers (
  ctx: {
    nodeId: string,
    node: DependenciesTreeNode,
    parentPkgs: ParentRefs,
    dependenciesTree: DependenciesTree,
    prefix: string,
    strictPeerDependencies: boolean,
  },
): {
  [alias: string]: string,
} {
  const resolvedPeers: {[alias: string]: string} = {}
  for (const peerName in ctx.node.resolvedPackage.peerDependencies) { // tslint:disable-line:forin
    const peerVersionRange = ctx.node.resolvedPackage.peerDependencies[peerName]

    let resolved = ctx.parentPkgs[peerName]

    if (!resolved || resolved.nodeId && !ctx.dependenciesTree[resolved.nodeId].installable) {
      try {
        const { version } = importFrom(ctx.prefix, `${peerName}/package.json`)
        resolved = {
          depth: -1,
          version,
        }
      } catch (err) {
        if (
          ctx.node.resolvedPackage.additionalInfo.peerDependenciesMeta &&
          ctx.node.resolvedPackage.additionalInfo.peerDependenciesMeta[peerName] &&
          ctx.node.resolvedPackage.additionalInfo.peerDependenciesMeta[peerName].optional === true
        ) {
          continue
        }
        const friendlyPath = nodeIdToFriendlyPath(ctx.nodeId, ctx.dependenciesTree)
        const message = oneLine`
          ${friendlyPath ? `${friendlyPath}: ` : ''}${packageFriendlyId(ctx.node.resolvedPackage)}
          requires a peer of ${peerName}@${peerVersionRange} but none was installed.`
        if (ctx.strictPeerDependencies) {
          throw new PnpmError('MISSING_PEER_DEPENDENCY', message)
        }
        logger.warn({
          message,
          prefix: ctx.prefix,
        })
        continue
      }
    }

    if (!semver.satisfies(resolved.version, peerVersionRange)) {
      const friendlyPath = nodeIdToFriendlyPath(ctx.nodeId, ctx.dependenciesTree)
      const message = oneLine`
        ${friendlyPath ? `${friendlyPath}: ` : ''}${packageFriendlyId(ctx.node.resolvedPackage)}
        requires a peer of ${peerName}@${peerVersionRange} but version ${resolved.version} was installed.`
      if (ctx.strictPeerDependencies) {
        throw new PnpmError('INVALID_PEER_DEPENDENCY', message)
      }
      logger.warn({
        message,
        prefix: ctx.prefix,
      })
    }

    if (resolved.depth === ctx.node.depth + 1) {
      // if the resolved package is a regular dependency of the package
      // then there is no need to link it in
      continue
    }

    if (resolved && resolved.nodeId) resolvedPeers[peerName] = resolved.nodeId
  }
  return resolvedPeers
}

function packageFriendlyId (manifest: {name: string, version: string}) {
  return `${manifest.name}@${manifest.version}`
}

function nodeIdToFriendlyPath (nodeId: string, dependenciesTree: DependenciesTree) {
  const parts = splitNodeId(nodeId).slice(1, -2)
  const result = R.scan((prevNodeId, pkgId) => createNodeId(prevNodeId, pkgId), '>', parts)
    .slice(2)
    .map((nid) => dependenciesTree[nid].resolvedPackage.name)
    .join(' > ')
  return result
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

function toPkgByName (nodes: Array<{alias: string, nodeId: string, node: DependenciesTreeNode}>): ParentRefs {
  const pkgsByName: ParentRefs = {}
  for (const { alias, node, nodeId } of nodes) {
    pkgsByName[alias] = {
      depth: node.depth,
      nodeId,
      version: node.resolvedPackage.version,
    }
  }
  return pkgsByName
}

function createPeersFolderSuffix (peers: Array<{name: string, version: string}>) {
  const folderName = peers.map(({ name, version }) => `${name.replace('/', '+')}@${version}`).sort().join('+')

  // We don't want the folder name to get too long.
  // Otherwise, an ENAMETOOLONG error might happen.
  // see: https://github.com/pnpm/pnpm/issues/977
  //
  // A bigger limit might be fine but the md5 hash will be 32 symbols,
  // so for consistency's sake, we go with 32.
  if (folderName.length > 32) {
    return `_${crypto.createHash('md5').update(folderName).digest('hex')}`
  }
  return `_${folderName}`
}
