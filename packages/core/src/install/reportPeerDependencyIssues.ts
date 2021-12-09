import renderPeerIssues from '@pnpm/render-peer-issues'
import PnpmError from '@pnpm/error'
import logger from '@pnpm/logger'
import { PeerDependencyIssues } from '@pnpm/resolve-dependencies'

export default function (
  peerDependencyIssues: PeerDependencyIssues,
  opts: {
    lockfileDir: string
    strictPeerDependencies: boolean
  }
) {
  const peerIssuesTree = renderPeerIssues(peerDependencyIssues)
  if (peerIssuesTree === '') return
  if (opts.strictPeerDependencies) {
    throw new PnpmError('PEER_DEPENDENCY', 'Unmet peer dependencies', { hint: peerIssuesTree })
  }
  logger.warn({
    message: `Unmet peer dependencies\n${peerIssuesTree}`,
    prefix: opts.lockfileDir,
  })
}
