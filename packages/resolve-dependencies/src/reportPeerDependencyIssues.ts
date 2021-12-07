import path from 'path'
import PnpmError from '@pnpm/error'
import logger from '@pnpm/logger'
import scan from 'ramda/src/scan'
import {
  createNodeId,
  splitNodeId,
} from './nodeIdUtils'
import {
  DependenciesTree,
  ResolvedPackage,
} from './resolveDependencies'
import { PartialResolvedPackage, PeerDependencyIssue } from './resolvePeers'

export default function<T extends PartialResolvedPackage> (
  peerDependencyIssues: PeerDependencyIssue[],
  opts: {
    dependenciesTree: DependenciesTree<T>
    lockfileDir: string
    strictPeerDependencies: boolean
  }
) {
  for (const peerDependencyIssue of peerDependencyIssues) {
    const message = peerDependencyIssueMessage({
      peerDependencyIssue,
      ...opts,
    })
    if (opts.strictPeerDependencies) {
      const code = peerDependencyIssue.foundPeerVersion ? 'INVALID_PEER_DEPENDENCY' : 'MISSING_PEER_DEPENDENCY'
      const err = new PnpmError(code, message)
      if (peerDependencyIssues.length === 1) {
        throw err
      }
      logger.error(err)
    } else {
      logger.warn({
        message,
        prefix: peerDependencyIssue.rootDir,
      })
    }
  }
  if (opts.strictPeerDependencies) {
    throw new PnpmError('PEER_DEPENDENCY', `${peerDependencyIssues.length} peer dependency issues found.`)
  }
}

function peerDependencyIssueMessage<T extends PartialResolvedPackage> (
  opts: {
    dependenciesTree: DependenciesTree<T>
    lockfileDir: string
    peerDependencyIssue: PeerDependencyIssue
  }
) {
  const friendlyPath = nodeIdToFriendlyPath({
    dependenciesTree: opts.dependenciesTree,
    lockfileDir: opts.lockfileDir,
    nodeId: opts.peerDependencyIssue.nodeId,
    rootDir: opts.peerDependencyIssue.rootDir,
  })
  if (opts.peerDependencyIssue.foundPeerVersion) {
    return `${friendlyPath ? `${friendlyPath}: ` : ''}${packageFriendlyId(opts.peerDependencyIssue.pkg)} \
requires a peer of ${opts.peerDependencyIssue.wantedPeer.name}@${opts.peerDependencyIssue.wantedPeer.range} but version ${opts.peerDependencyIssue.foundPeerVersion} was installed.`
  }
  return `${friendlyPath ? `${friendlyPath}: ` : ''}${packageFriendlyId(opts.peerDependencyIssue.pkg)} \
requires a peer of ${opts.peerDependencyIssue.wantedPeer.name}@${opts.peerDependencyIssue.wantedPeer.range} but none was installed.`
}

function packageFriendlyId (manifest: {name: string, version: string}) {
  return `${manifest.name}@${manifest.version}`
}

function nodeIdToFriendlyPath<T extends PartialResolvedPackage> (
  {
    dependenciesTree,
    lockfileDir,
    nodeId,
    rootDir,
  }: {
    dependenciesTree: DependenciesTree<T>
    lockfileDir: string
    nodeId: string
    rootDir: string
  }
) {
  const parts = splitNodeId(nodeId).slice(0, -1)
  const result = scan((prevNodeId, pkgId) => createNodeId(prevNodeId, pkgId), '>', parts)
    .slice(2)
    .map((nid) => (dependenciesTree[nid].resolvedPackage as ResolvedPackage).name)
  const projectPath = path.relative(lockfileDir, rootDir)
  if (projectPath) {
    result.unshift(projectPath)
  }
  return result.join(' > ')
}
