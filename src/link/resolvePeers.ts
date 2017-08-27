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
import Rx = require('@reactivex/rxjs')

type PartiallyResolvedNodeContainer = {
  // a node ID is the join of the package's keypath with a colon
  // E.g., a subdeps node ID which parent is `foo` will be
  // registry.npmjs.org/foo/1.0.0:registry.npmjs.org/bar/1.0.0
  nodeId: string,
  node: PartiallyResolvedNode,
  depth: number,
  isRepeated: boolean,
  isCircular: boolean,
}

export type ResolvedNode = {
  name: string,
  // at this point the version is really needed only for logging
  version: string,
  hasBundledDependencies: boolean,
  hasBins: boolean,
  path: string,
  modules: string,
  fetchingFiles: Promise<PackageContentInfo>,
  resolution: Resolution,
  hardlinkedLocation: string,
  children$: Rx.Observable<ResolvedNode>,
  // an independent package is a package that
  // has neither regular nor peer dependencies
  independent: boolean,
  optionalDependencies: Set<string>,
  depth: number,
  absolutePath: string,
  dev: boolean,
  optional: boolean,
  pkgId: string,
  installable: boolean,
}

// All the direct children are resolved but the peer dependencies are not
type PartiallyResolvedNode = ResolvedNode & {
  peerNodeIds$: Rx.Observable<string>,
}

export type Map<T> = {
  [nodeId: string]: T
}

export default function (
  tree: TreeNodeMap,
  rootNodeId$: Rx.Observable<string>,
  // only the top dependencies that were already installed
  // to avoid warnings about unresolved peer dependencies
  topParent$: Rx.Observable<{name: string, version: string}>,
  independentLeaves: boolean,
  nodeModules: string,
  opts: {
    nonDevPackageIds: Set<string>,
    nonOptionalPackageIds: Set<string>,
  }
): {
  resolvedNode$: Rx.Observable<ResolvedNode>,
  rootResolvedNode$: Rx.Observable<ResolvedNode>,
} {
  const parentPkg$ = Rx.Observable.concat(
    toRef$(rootNodeId$, tree),
    topParent$.map(parent => ({
      name: parent.name,
      version: parent.version,
      depth: 0,
    }))
  )
  .shareReplay(Infinity)

  const result = resolvePeersOfChildren(rootNodeId$, parentPkg$, {
    tree,
    purePkgs: new Set(),
    partiallyResolvedNodeMap: {},
    independentLeaves,
    nodeModules,
    nonDevPackageIds: opts.nonDevPackageIds,
    nonOptionalPackageIds: opts.nonOptionalPackageIds,
  })

  return {
    resolvedNode$: result.partiallyResolvedNodeContainer$
      .mergeMap(container => {
        if (container.isRepeated) {
          return Rx.Observable.empty<PartiallyResolvedNodeContainer>()
        }
        if (container.isCircular) {
          return result.partiallyResolvedNodeContainer$
            .find(nextContainer => !nextContainer.isCircular &&
              container.node.pkgId === nextContainer.node.pkgId &&
              container.nodeId.startsWith(nextContainer.nodeId))
            .mergeMap(nextContainer => {
              if (!nextContainer) {
                return Rx.Observable.of(container)
              }
              if (nextContainer.node.absolutePath === container.node.absolutePath) {
                return Rx.Observable.of(nextContainer)
              }
              return Rx.Observable.from([nextContainer, container])
            })
        }
        return Rx.Observable.of(container)
      })
      .distinct(container => container.node.absolutePath) /// this is bad.....
      .map(container => Object.assign(container.node, {
        children$: container.node.children$.merge(
          container.node.peerNodeIds$.mergeMap(peerNodeId =>
            result.partiallyResolvedNodeContainer$
              .single(childNode => childNode.nodeId === peerNodeId)
              .map(childNode => childNode.node))
          ),
      }))
      .shareReplay(Infinity),
      rootResolvedNode$: result.partiallyResolvedChildContainer$.map(container => container.node),
  }
}

function resolvePeersOfNode (
  nodeId: string,
  parentParentPkgs$: Rx.Observable<ParentRef>,
  ctx: {
    tree: TreeNodeMap,
    partiallyResolvedNodeMap: Map<PartiallyResolvedNode>,
    independentLeaves: boolean,
    nodeModules: string,
    nonDevPackageIds: Set<string>,
    nonOptionalPackageIds: Set<string>,
    purePkgs: Set<string>, // pure packages are those that don't rely on externally resolved peers
  }
): {
  externalPeer$: Rx.Observable<string>,
  partiallyResolvedChildContainer$: Rx.Observable<PartiallyResolvedNodeContainer>,
  partiallyResolvedNodeContainer$: Rx.Observable<PartiallyResolvedNodeContainer>,
} {
  const node = ctx.tree[nodeId]
  if (ctx.purePkgs.has(node.pkg.id)) {
    const absolutePath = node.pkg.id
    return {
      externalPeer$: Rx.Observable.empty(),
      partiallyResolvedChildContainer$: Rx.Observable.of({
        depth: node.depth,
        node: ctx.partiallyResolvedNodeMap[absolutePath],
        nodeId: node.nodeId,
        isRepeated: true,
        isCircular: node.isCircular,
      }),
      partiallyResolvedNodeContainer$: Rx.Observable.empty(),
    }
  }

  const parentPkgs$ = Rx.Observable.concat(
    toRef$(node.children$, ctx.tree),
    parentParentPkgs$
  )
  .shareReplay(Infinity)
  const result = resolvePeersOfChildren(node.children$, parentPkgs$, ctx)

  // external peers are peers resolved from parent dependencies
  const childsExternalPeer$ = result.externalPeer$
    .filter(unresolvedPeerNodeId => unresolvedPeerNodeId !== nodeId)

  const ownExternalPeer$ = resolvePeers(node, parentPkgs$, ctx.tree).shareReplay(Infinity)

  const externalPeer$ = childsExternalPeer$.merge(ownExternalPeer$)

  const partiallyResolvedChildContainer$ = externalPeer$
    .toArray()
    .map(externalPeers => resolveNode(ctx, node, externalPeers, ownExternalPeer$, result.partiallyResolvedChildContainer$))
    .shareReplay(1)

  return {
    externalPeer$,
    partiallyResolvedChildContainer$,
    partiallyResolvedNodeContainer$: result.partiallyResolvedNodeContainer$,
  }
}

function resolveNode (
  ctx: {
    tree: TreeNodeMap,
    partiallyResolvedNodeMap: Map<PartiallyResolvedNode>,
    independentLeaves: boolean,
    nodeModules: string,
    nonDevPackageIds: Set<string>,
    nonOptionalPackageIds: Set<string>,
    purePkgs: Set<string>,
  },
  node: TreeNode,
  externalPeers: string[],
  ownExternalPeer$: Rx.Observable<string>,
  partiallyResolvedChildContainer$: Rx.Observable<PartiallyResolvedNodeContainer>
) {
  let modules: string
  let absolutePath: string
  const localLocation = path.join(ctx.nodeModules, `.${pkgIdToFilename(node.pkg.id)}`)
  if (!externalPeers.length) {
    ctx.purePkgs.add(node.pkg.id)
    modules = path.join(localLocation, 'node_modules')
    absolutePath = node.pkg.id
  } else {
    const peersFolder = createPeersFolderName(R.props<TreeNode>(externalPeers, ctx.tree).map(node => node.pkg))
    modules = path.join(localLocation, peersFolder, 'node_modules')
    absolutePath = `${node.pkg.id}/${peersFolder}`
  }

  if (ctx.partiallyResolvedNodeMap[absolutePath] &&
    // TODO: check isCircular instead of depth here
    ctx.partiallyResolvedNodeMap[absolutePath].depth <= node.depth) {

    return {
      depth: node.depth,
      node: ctx.partiallyResolvedNodeMap[absolutePath],
      nodeId: node.nodeId,
      isRepeated: true,
      isCircular: node.isCircular,
    }
  }

  const independent = ctx.independentLeaves && !node.pkg.childrenCount && R.isEmpty(node.pkg.peerDependencies)
  const pathToUnpacked = path.join(node.pkg.path, 'node_modules', node.pkg.name)
  const hardlinkedLocation = !independent
    ? path.join(modules, node.pkg.name)
    : pathToUnpacked
  ctx.partiallyResolvedNodeMap[absolutePath] = {
    name: node.pkg.name,
    version: node.pkg.version,
    hasBundledDependencies: node.pkg.hasBundledDependencies,
    hasBins: node.pkg.hasBins,
    fetchingFiles: node.pkg.fetchingFiles,
    resolution: node.pkg.resolution,
    path: pathToUnpacked,
    modules,
    hardlinkedLocation,
    independent,
    optionalDependencies: node.pkg.optionalDependencies,
    children$: partiallyResolvedChildContainer$.map(childNode => childNode.node),
    peerNodeIds$: ownExternalPeer$,
    depth: node.depth,
    absolutePath,
    dev: !ctx.nonDevPackageIds.has(node.pkg.id),
    optional: !ctx.nonOptionalPackageIds.has(node.pkg.id),
    pkgId: node.pkg.id,
    installable: node.installable,
  }
  return {
    depth: node.depth,
    node: ctx.partiallyResolvedNodeMap[absolutePath],
    nodeId: node.nodeId,
    isRepeated: false,
    isCircular: node.isCircular,
  }
}

function arrayToSet<T> (arr: T[]): Set<T> {
  return new Set(arr)
}

function resolvePeersOfChildren (
  children$: Rx.Observable<string>,
  parentPkgs$: Rx.Observable<ParentRef>,
  ctx: {
    tree: {[nodeId: string]: TreeNode},
    partiallyResolvedNodeMap: Map<PartiallyResolvedNode>,
    independentLeaves: boolean,
    nodeModules: string,
    nonDevPackageIds: Set<string>,
    nonOptionalPackageIds: Set<string>,
    purePkgs: Set<string>,
  }
): {
  externalPeer$: Rx.Observable<string>,
  partiallyResolvedChildContainer$: Rx.Observable<PartiallyResolvedNodeContainer>,
  partiallyResolvedNodeContainer$: Rx.Observable<PartiallyResolvedNodeContainer>,
} {
  const result = children$
    .map(child => resolvePeersOfNode(child, parentPkgs$, ctx))
    .map(result => ({
      externalPeer$: result.externalPeer$,
      partiallyResolvedChildContainer$: result.partiallyResolvedChildContainer$,
      partiallyResolvedNodeContainer$: Rx.Observable.merge(
        result.partiallyResolvedNodeContainer$,
        result.partiallyResolvedChildContainer$
      ),
    }))
    .shareReplay(Infinity)

  return {
    externalPeer$: result.mergeMap(result => result.externalPeer$),
    partiallyResolvedChildContainer$: result.mergeMap(result => result.partiallyResolvedChildContainer$).shareReplay(Infinity),
    partiallyResolvedNodeContainer$: result.mergeMap(result => result.partiallyResolvedNodeContainer$),
  }
}

function resolvePeers (
  node: TreeNode,
  parentPkg$: Rx.Observable<ParentRef>,
  tree: TreeNodeMap
): Rx.Observable<string> {
  return Rx.Observable.from(R.keys(node.pkg.peerDependencies))
    .mergeMap(peerName => {
      const peerVersionRange = node.pkg.peerDependencies[peerName]

      return parentPkg$
        .find(parentPkg => parentPkg.name === peerName)
        .mergeMap(resolved => {
          if (!resolved || resolved.nodeId && !tree[resolved.nodeId].installable) {
            logger.warn(`${node.pkg.id} requires a peer of ${peerName}@${peerVersionRange} but none was installed.`)
            return Rx.Observable.empty()
          }

          if (!semver.satisfies(resolved.version, peerVersionRange)) {
            logger.warn(`${node.pkg.id} requires a peer of ${peerName}@${peerVersionRange} but version ${resolved.version} was installed.`)
          }

          if (resolved.depth === node.depth + 1) {
            // if the peer dependency is resolved from a regular dependency of the package
            // then there is no need to link it in
            return Rx.Observable.empty()
          }

          if (resolved && resolved.nodeId) return Rx.Observable.of(resolved.nodeId)

          return Rx.Observable.empty()
        })
    })
}

type ParentRef = {
  name: string,
  version: string,
  depth: number,
  // this is null only for already installed top dependencies
  nodeId?: string,
}

function toRef$ (children$: Rx.Observable<string>, tree: TreeNodeMap): Rx.Observable<ParentRef> {
  return children$
    .map(nodeId => tree[nodeId])
    .map(node => ({
      name: node.pkg.name,
      version: node.pkg.version,
      nodeId: node.nodeId,
      depth: node.depth,
    }))
}

function createPeersFolderName(peers: InstalledPackage[]) {
  return peers.map(peer => `${peer.name.replace('/', '!')}@${peer.version}`).sort().join('+')
}
