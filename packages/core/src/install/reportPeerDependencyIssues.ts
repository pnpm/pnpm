import PnpmError from '@pnpm/error'
import logger from '@pnpm/logger'
import { PeerDependencyIssue, PeerDependencyIssueLocation } from '@pnpm/resolve-dependencies'

export default function (
  peerDependencyIssues: PeerDependencyIssue[],
  opts: {
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

function peerDependencyIssueMessage (
  opts: {
    lockfileDir: string
    peerDependencyIssue: PeerDependencyIssue
  }
) {
  const friendlyPath = locationToFriendlyPath(opts.peerDependencyIssue.location)
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

function locationToFriendlyPath (location: PeerDependencyIssueLocation) {
  const result = location.parents.map(({ name }) => name)
  if (location.projectPath) {
    result.unshift(location.projectPath)
  }
  return result.join(' > ')
}
