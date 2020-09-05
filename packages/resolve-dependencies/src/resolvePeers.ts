import PnpmError from '@pnpm/error'
import logger from '@pnpm/logger'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import { Resolution } from '@pnpm/resolver-base'
import { PackageFilesResponse } from '@pnpm/store-controller-types'
import { Dependencies, DependencyManifest } from '@pnpm/types'
import {
  createNodeId,
  splitNodeId,
} from './nodeIdUtils'
import {
  DependenciesTree,
  DependenciesTreeNode,
  ResolvedPackage,
} from './resolveDependencies'
import crypto = require('crypto')
import importFrom = require('import-from')
import path = require('path')
import R = require('ramda')
import semver = require('semver')

export interface DependenciesGraphNode {
  name: string
  // at this point the version is really needed only for logging
  version: string
  hasBin: boolean
  hasBundledDependencies: boolean
  modules: string
  fetchingBundledManifest?: () => Promise<DependencyManifest>
  fetchingFiles: () => Promise<PackageFilesResponse>
  filesIndexFile: string
  resolution: Resolution
  dir: string
  children: {[alias: string]: string}
  optionalDependencies: Set<string>
  depth: number
  depPath: string
  prod: boolean
  dev: boolean
  optional: boolean
  packageId: string
  installable: boolean
  additionalInfo: {
    deprecated?: string
    peerDependencies?: Dependencies
    bundleDependencies?: string[]
    bundledDependencies?: string[]
    engines?: {
      node?: string
      npm?: string
    }
    cpu?: string[]
    os?: string[]
  }
  isBuilt?: boolean
  requiresBuild?: boolean
  prepare: boolean
  isPure: boolean
}

export interface DependenciesGraph {
  [depPath: string]: DependenciesGraphNode
}

export default function (
  opts: {
    projects: Array<{
      directNodeIdsByAlias: {[alias: string]: string}
      // only the top dependencies that were already installed
      // to avoid warnings about unresolved peer dependencies
      topParents: Array<{name: string, version: string}>
      rootDir: string // is only needed for logging
      id: string
    }>
    dependenciesTree: DependenciesTree
    virtualStoreDir: string
    lockfileDir: string
    strictPeerDependencies: boolean
  }
): {
    dependenciesGraph: DependenciesGraph
    projectsDirectPathsByAlias: {[id: string]: {[alias: string]: string}}
  } {
  const depGraph: DependenciesGraph = {}
  const pathsByNodeId = {}

  for (const { directNodeIdsByAlias, topParents, rootDir } of opts.projects) {
    const pkgsByName = Object.assign(
      R.fromPairs(
        topParents.map(({ name, version }: {name: string, version: string}): R.KeyValuePair<string, ParentRef> => [
          name,
          {
            depth: 0,
            version,
          },
        ])
      ),
      toPkgByName(
        Object
          .keys(directNodeIdsByAlias)
          .map((alias) => ({
            alias,
            node: opts.dependenciesTree[directNodeIdsByAlias[alias]],
            nodeId: directNodeIdsByAlias[alias],
          }))
      )
    )

    resolvePeersOfChildren(directNodeIdsByAlias, pkgsByName, {
      dependenciesTree: opts.dependenciesTree,
      depGraph,
      lockfileDir: opts.lockfileDir,
      pathsByNodeId,
      peersCache: new Map(),
      purePkgs: new Set(),
      rootDir,
      strictPeerDependencies: opts.strictPeerDependencies,
      virtualStoreDir: opts.virtualStoreDir,
    })
  }

  R.values(depGraph).forEach((node) => {
    node.children = R.keys(node.children).reduce((acc, alias) => {
      acc[alias] = pathsByNodeId[node.children[alias]] ?? node.children[alias]
      return acc
    }, {})
  })

  const projectsDirectPathsByAlias: {[id: string]: {[alias: string]: string}} = {}
  for (const { directNodeIdsByAlias, id } of opts.projects) {
    projectsDirectPathsByAlias[id] = R.keys(directNodeIdsByAlias).reduce((rootPathsByAlias, alias) => {
      rootPathsByAlias[alias] = pathsByNodeId[directNodeIdsByAlias[alias]]
      return rootPathsByAlias
    }, {})
  }
  return {
    dependenciesGraph: depGraph,
    projectsDirectPathsByAlias,
  }
}

interface PeersCacheItem {
  depPath: string
  resolvedPeers: Array<[string, string]>
  missingPeers: string[]
}

type PeersCache = Map<string, PeersCacheItem[]>

interface PeersResolution {
  missingPeers: string[]
  resolvedPeers: Record<string, string>
}

function resolvePeersOfNode (
  nodeId: string,
  parentParentPkgs: ParentRefs,
  ctx: {
    dependenciesTree: DependenciesTree
    pathsByNodeId: {[nodeId: string]: string}
    depGraph: DependenciesGraph
    virtualStoreDir: string
    peersCache: PeersCache
    purePkgs: Set<string> // pure packages are those that don't rely on externally resolved peers
    rootDir: string
    lockfileDir: string
    strictPeerDependencies: boolean
  }
): PeersResolution {
  const node = ctx.dependenciesTree[nodeId]
  if (node.depth === -1) return { resolvedPeers: {}, missingPeers: [] }
  const resolvedPackage = node.resolvedPackage as ResolvedPackage
  if (ctx.purePkgs.has(resolvedPackage.depPath) && ctx.depGraph[resolvedPackage.depPath].depth <= node.depth) {
    ctx.pathsByNodeId[nodeId] = resolvedPackage.depPath
    return { resolvedPeers: {}, missingPeers: [] }
  }
  if (typeof node.children === 'function') {
    node.children = node.children()
  }
  const children = node.children
  const parentPkgs = R.isEmpty(children)
    ? parentParentPkgs
    : {
      ...parentParentPkgs,
      ...toPkgByName(
        Object.keys(children).map((alias) => ({
          alias,
          node: ctx.dependenciesTree[children[alias]],
          nodeId: children[alias],
        }))
      ),
    }
  const hit = ctx.peersCache.get(resolvedPackage.depPath)?.find((cache) =>
    cache.resolvedPeers
      .every(([name, cachedNodeId]) => {
        const parentPkgNodeId = parentPkgs[name]?.nodeId
        if (!parentPkgNodeId || !cachedNodeId) return false
        if (parentPkgs[name].nodeId === cachedNodeId) return true
        if (
          ctx.pathsByNodeId[cachedNodeId] &&
          ctx.pathsByNodeId[cachedNodeId] === ctx.pathsByNodeId[parentPkgs[name].nodeId!]
        ) return true
        const parentDepPath = (ctx.dependenciesTree[parentPkgNodeId].resolvedPackage as ResolvedPackage).depPath
        if (!ctx.purePkgs.has(parentDepPath)) return false
        const cachedDepPath = (ctx.dependenciesTree[cachedNodeId].resolvedPackage as ResolvedPackage).depPath
        return parentDepPath === cachedDepPath
      }) && cache.missingPeers.every((missingPeer) => !parentPkgs[missingPeer])
  )
  if (hit) {
    ctx.pathsByNodeId[nodeId] = hit.depPath
    ctx.depGraph[hit.depPath].depth = Math.min(ctx.depGraph[hit.depPath].depth, node.depth)
    return {
      missingPeers: hit.missingPeers,
      resolvedPeers: R.fromPairs(hit.resolvedPeers),
    }
  }

  const {
    resolvedPeers: unknownResolvedPeersOfChildren,
    missingPeers: missingPeersOfChildren,
  } = resolvePeersOfChildren(children, parentPkgs, ctx)

  const { resolvedPeers, missingPeers } = R.isEmpty(resolvedPackage.peerDependencies)
    ? { resolvedPeers: {}, missingPeers: [] }
    : resolvePeers({
      currentDepth: node.depth,
      dependenciesTree: ctx.dependenciesTree,
      nodeId,
      parentPkgs,
      resolvedPackage,
      rootDir: ctx.rootDir,
      strictPeerDependencies: ctx.strictPeerDependencies,
    })

  const allResolvedPeers = Object.assign(unknownResolvedPeersOfChildren, resolvedPeers)
  const allMissingPeers = Array.from(new Set([...missingPeersOfChildren, ...missingPeers]))

  let modules: string
  let depPath: string
  const localLocation = path.join(ctx.virtualStoreDir, pkgIdToFilename(resolvedPackage.depPath, ctx.lockfileDir))
  const isPure = R.isEmpty(allResolvedPeers)
  if (isPure) {
    modules = path.join(localLocation, 'node_modules')
    depPath = resolvedPackage.depPath
    if (R.isEmpty(resolvedPackage.peerDependencies)) {
      ctx.purePkgs.add(resolvedPackage.depPath)
    }
  } else {
    const peersFolderSuffix = createPeersFolderSuffix(
      Object.keys(allResolvedPeers).map((alias) => ({
        name: alias,
        version: ctx.dependenciesTree[allResolvedPeers[alias]].resolvedPackage.version,
      })))
    modules = path.join(`${localLocation}${peersFolderSuffix}`, 'node_modules')
    depPath = `${resolvedPackage.depPath}${peersFolderSuffix}`
  }
  if (!isPure || !R.isEmpty(resolvedPackage.peerDependencies)) {
    const cache = {
      missingPeers: allMissingPeers,
      depPath,
      resolvedPeers: Object.entries(allResolvedPeers),
    }
    if (ctx.peersCache.has(resolvedPackage.depPath)) {
      ctx.peersCache.get(resolvedPackage.depPath)!.push(cache)
    } else {
      ctx.peersCache.set(resolvedPackage.depPath, [cache])
    }
  }

  ctx.pathsByNodeId[nodeId] = depPath
  if (!ctx.depGraph[depPath] || ctx.depGraph[depPath].depth > node.depth) {
    const dir = path.join(modules, resolvedPackage.name)

    const unknownPeers = Object.keys(unknownResolvedPeersOfChildren)
    if (unknownPeers.length) {
      if (!resolvedPackage.additionalInfo.peerDependencies) {
        resolvedPackage.additionalInfo.peerDependencies = {}
      }
      for (const unknownPeer of unknownPeers) {
        if (!resolvedPackage.additionalInfo.peerDependencies[unknownPeer]) {
          resolvedPackage.additionalInfo.peerDependencies[unknownPeer] = '*'
        }
      }
    }
    ctx.depGraph[depPath] = {
      additionalInfo: resolvedPackage.additionalInfo,
      children: Object.assign(
        getPreviouslyResolvedChildren(nodeId, ctx.dependenciesTree),
        children,
        resolvedPeers
      ),
      depPath,
      depth: node.depth,
      dev: resolvedPackage.dev,
      dir,
      fetchingBundledManifest: resolvedPackage.fetchingBundledManifest,
      fetchingFiles: resolvedPackage.fetchingFiles,
      filesIndexFile: resolvedPackage.filesIndexFile,
      hasBin: resolvedPackage.hasBin,
      hasBundledDependencies: resolvedPackage.hasBundledDependencies,
      installable: node.installable,
      isPure,
      modules,
      name: resolvedPackage.name,
      optional: resolvedPackage.optional,
      optionalDependencies: resolvedPackage.optionalDependencies,
      packageId: resolvedPackage.id,
      prepare: resolvedPackage.prepare,
      prod: resolvedPackage.prod,
      requiresBuild: resolvedPackage.requiresBuild,
      resolution: resolvedPackage.resolution,
      version: resolvedPackage.version,
    }
  }
  return { resolvedPeers: allResolvedPeers, missingPeers: allMissingPeers }
}

// When a package has itself in the subdependencies, so there's a cycle,
// pnpm will break the cycle, when it first repeats itself.
// However, when the cycle is broken up, the last repeated package is removed
// from the dependencies of the parent package.
// So we need to merge all the children of all the parent packages with same ID as the resolved package.
// This way we get all the children that were removed, when ending cycles.
function getPreviouslyResolvedChildren (nodeId: string, dependenciesTree: DependenciesTree) {
  const parentIds = splitNodeId(nodeId)
  const ownId = parentIds.pop()
  const allChildren = {}

  if (!ownId || !parentIds.includes(ownId)) return allChildren

  const nodeIdChunks = parentIds.join('>').split(ownId)
  nodeIdChunks.pop()
  nodeIdChunks.reduce((accNodeId, part) => {
    accNodeId += `${part}${ownId}`
    const parentNode = dependenciesTree[`${accNodeId}>`]
    Object.assign(
      allChildren,
      typeof parentNode.children === 'function' ? parentNode.children() : parentNode.children
    )
    return accNodeId
  }, '>')
  return allChildren
}

function resolvePeersOfChildren (
  children: {
    [alias: string]: string
  },
  parentPkgs: ParentRefs,
  ctx: {
    pathsByNodeId: {[nodeId: string]: string}
    peersCache: PeersCache
    virtualStoreDir: string
    purePkgs: Set<string>
    depGraph: DependenciesGraph
    dependenciesTree: DependenciesTree
    rootDir: string
    lockfileDir: string
    strictPeerDependencies: boolean
  }
): PeersResolution {
  const allResolvedPeers: Record<string, string> = {}
  const allMissingPeers = new Set<string>()

  for (const childNodeId of R.values(children)) {
    const { resolvedPeers, missingPeers } = resolvePeersOfNode(childNodeId, parentPkgs, ctx)
    Object.assign(allResolvedPeers, resolvedPeers)
    missingPeers.forEach((missingPeer) => allMissingPeers.add(missingPeer))
  }

  const unknownResolvedPeersOfChildren = R.keys(allResolvedPeers)
    .filter((alias) => !children[alias])
    .reduce((acc, peer) => {
      acc[peer] = allResolvedPeers[peer]
      return acc
    }, {})

  return { resolvedPeers: unknownResolvedPeersOfChildren, missingPeers: Array.from(allMissingPeers) }
}

function resolvePeers (
  ctx: {
    currentDepth: number
    nodeId: string
    parentPkgs: ParentRefs
    resolvedPackage: ResolvedPackage
    dependenciesTree: DependenciesTree
    rootDir: string
    strictPeerDependencies: boolean
  }
): PeersResolution {
  const resolvedPeers: {[alias: string]: string} = {}
  const missingPeers = []
  for (const peerName in ctx.resolvedPackage.peerDependencies) { // eslint-disable-line:forin
    const peerVersionRange = ctx.resolvedPackage.peerDependencies[peerName]

    let resolved = ctx.parentPkgs[peerName]

    if (!resolved || resolved.nodeId && !ctx.dependenciesTree[resolved.nodeId].installable) {
      try {
        const { version } = importFrom(ctx.rootDir, `${peerName}/package.json`) as { version: string }
        resolved = {
          depth: -1,
          version,
        }
      } catch (err) {
        missingPeers.push(peerName)
        if (
          ctx.resolvedPackage.additionalInfo.peerDependenciesMeta?.[peerName]?.optional === true
        ) {
          continue
        }
        const friendlyPath = nodeIdToFriendlyPath(ctx.nodeId, ctx.dependenciesTree)
        const message = `${friendlyPath ? `${friendlyPath}: ` : ''}${packageFriendlyId(ctx.resolvedPackage)} \
requires a peer of ${peerName}@${peerVersionRange} but none was installed.`
        if (ctx.strictPeerDependencies) {
          throw new PnpmError('MISSING_PEER_DEPENDENCY', message)
        }
        logger.warn({
          message,
          prefix: ctx.rootDir,
        })
        continue
      }
    }

    if (!semver.satisfies(resolved.version, peerVersionRange)) {
      const friendlyPath = nodeIdToFriendlyPath(ctx.nodeId, ctx.dependenciesTree)
      const message = `${friendlyPath ? `${friendlyPath}: ` : ''}${packageFriendlyId(ctx.resolvedPackage)} \
requires a peer of ${peerName}@${peerVersionRange} but version ${resolved.version} was installed.`
      if (ctx.strictPeerDependencies) {
        throw new PnpmError('INVALID_PEER_DEPENDENCY', message)
      }
      logger.warn({
        message,
        prefix: ctx.rootDir,
      })
    }

    if (resolved.depth === ctx.currentDepth + 1) {
      // if the resolved package is a regular dependency of the package
      // then there is no need to link it in
      continue
    }

    if (resolved?.nodeId) resolvedPeers[peerName] = resolved.nodeId
  }
  return { resolvedPeers, missingPeers }
}

function packageFriendlyId (manifest: {name: string, version: string}) {
  return `${manifest.name}@${manifest.version}`
}

function nodeIdToFriendlyPath (nodeId: string, dependenciesTree: DependenciesTree) {
  const parts = splitNodeId(nodeId).slice(0, -1)
  const result = R.scan((prevNodeId, pkgId) => createNodeId(prevNodeId, pkgId), '>', parts)
    .slice(2)
    .map((nid) => (dependenciesTree[nid].resolvedPackage as ResolvedPackage).name)
    .join(' > ')
  return result
}

interface ParentRefs {
  [name: string]: ParentRef
}

interface ParentRef {
  version: string
  depth: number
  // this is null only for already installed top dependencies
  nodeId?: string
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
