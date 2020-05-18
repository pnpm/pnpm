import PnpmError from '@pnpm/error'
import logger from '@pnpm/logger'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import {
  createNodeId,
  DependenciesTree,
  DependenciesTreeNode,
  ResolvedPackage,
  splitNodeId,
} from '@pnpm/resolve-dependencies'
import { Resolution } from '@pnpm/resolver-base'
import { PackageFilesResponse } from '@pnpm/store-controller-types'
import { Dependencies, DependencyManifest } from '@pnpm/types'
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
  modules: string,
  fetchingBundledManifest?: () => Promise<DependencyManifest>,
  fetchingFiles: () => Promise<PackageFilesResponse>,
  filesIndexFile: string,
  resolution: Resolution,
  peripheralLocation: string,
  children: {[alias: string]: string},
  // an independent package is a package that
  // has neither regular nor peer dependencies
  independent: boolean,
  optionalDependencies: Set<string>,
  depth: number,
  depPath: string,
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
    projects: Array<{
      directNodeIdsByAlias: {[alias: string]: string},
      // only the top dependencies that were already installed
      // to avoid warnings about unresolved peer dependencies
      topParents: Array<{name: string, version: string}>,
      rootDir: string, // is only needed for logging
      id: string,
    }>,
    dependenciesTree: DependenciesTree,
    independentLeaves: boolean,
    virtualStoreDir: string,
    lockfileDir: string,
    strictPeerDependencies: boolean,
  }
): {
  depGraph: DependenciesGraph,
  projectsDirectPathsByAlias: {[id: string]: {[alias: string]: string}},
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
      independentLeaves: opts.independentLeaves,
      lockfileDir: opts.lockfileDir,
      pathsByNodeId,
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
    depGraph,
    projectsDirectPathsByAlias,
  }
}

function resolvePeersOfNode (
  nodeId: string,
  parentParentPkgs: ParentRefs,
  ctx: {
    dependenciesTree: DependenciesTree,
    pathsByNodeId: {[nodeId: string]: string},
    depGraph: DependenciesGraph,
    independentLeaves: boolean,
    virtualStoreDir: string,
    purePkgs: Set<string>, // pure packages are those that don't rely on externally resolved peers
    rootDir: string,
    lockfileDir: string,
    strictPeerDependencies: boolean,
  }
): {[alias: string]: string} {
  const node = ctx.dependenciesTree[nodeId]
  if (node.depth === -1) return {}
  const resolvedPackage = node.resolvedPackage as ResolvedPackage
  if (ctx.purePkgs.has(resolvedPackage.depPath) && ctx.depGraph[resolvedPackage.depPath].depth <= node.depth) {
    ctx.pathsByNodeId[nodeId] = resolvedPackage.depPath
    return {}
  }

  const children = typeof node.children === 'function' ? node.children() : node.children
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
  const unknownResolvedPeersOfChildren = resolvePeersOfChildren(children, parentPkgs, ctx)

  const resolvedPeers = R.isEmpty(resolvedPackage.peerDependencies)
    ? {}
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

  ctx.pathsByNodeId[nodeId] = depPath
  if (!ctx.depGraph[depPath] || ctx.depGraph[depPath].depth > node.depth) {
    const independent = ctx.independentLeaves && resolvedPackage.independent
    const centralLocation = resolvedPackage.engineCache || path.join(resolvedPackage.path, 'node_modules', resolvedPackage.name)
    const peripheralLocation = !independent
      ? path.join(modules, resolvedPackage.name)
      : centralLocation

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
      children: Object.assign(children, resolvedPeers),
      depPath,
      depth: node.depth,
      dev: resolvedPackage.dev,
      fetchingBundledManifest: resolvedPackage.fetchingBundledManifest,
      fetchingFiles: resolvedPackage.fetchingFiles,
      filesIndexFile: resolvedPackage.filesIndexFile,
      hasBin: resolvedPackage.hasBin,
      hasBundledDependencies: resolvedPackage.hasBundledDependencies,
      independent,
      installable: node.installable,
      isBuilt: !!resolvedPackage.engineCache,
      isPure,
      modules,
      name: resolvedPackage.name,
      optional: resolvedPackage.optional,
      optionalDependencies: resolvedPackage.optionalDependencies,
      packageId: resolvedPackage.id,
      peripheralLocation,
      prepare: resolvedPackage.prepare,
      prod: resolvedPackage.prod,
      requiresBuild: resolvedPackage.requiresBuild,
      resolution: resolvedPackage.resolution,
      version: resolvedPackage.version,
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
    pathsByNodeId: {[nodeId: string]: string},
    independentLeaves: boolean,
    virtualStoreDir: string,
    purePkgs: Set<string>,
    depGraph: DependenciesGraph,
    dependenciesTree: DependenciesTree,
    rootDir: string,
    lockfileDir: string,
    strictPeerDependencies: boolean,
  }
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
    currentDepth: number,
    nodeId: string,
    parentPkgs: ParentRefs,
    resolvedPackage: ResolvedPackage,
    dependenciesTree: DependenciesTree,
    rootDir: string,
    strictPeerDependencies: boolean,
  }
): {
  [alias: string]: string,
} {
  const resolvedPeers: {[alias: string]: string} = {}
  for (const peerName in ctx.resolvedPackage.peerDependencies) { // tslint:disable-line:forin
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
        if (
          ctx.resolvedPackage.additionalInfo.peerDependenciesMeta?.[peerName]?.optional === true
        ) {
          continue
        }
        const friendlyPath = nodeIdToFriendlyPath(ctx.nodeId, ctx.dependenciesTree)
        const message = oneLine`
          ${friendlyPath ? `${friendlyPath}: ` : ''}${packageFriendlyId(ctx.resolvedPackage)}
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
      const message = oneLine`
        ${friendlyPath ? `${friendlyPath}: ` : ''}${packageFriendlyId(ctx.resolvedPackage)}
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
  return resolvedPeers
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
