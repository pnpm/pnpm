import PnpmError from '@pnpm/error'
import { peerDependencyIssuesLogger } from '@pnpm/core-loggers'
import { PeerDependencyIssues } from '@pnpm/types'
import isEmpty from 'ramda/src/isEmpty'

export default function (
  peerDependencyIssues: PeerDependencyIssues,
  opts: {
    lockfileDir: string
    strictPeerDependencies: boolean
  }
) {
  if (
    isEmpty(peerDependencyIssues.bad) && (
      isEmpty(peerDependencyIssues.missing) ||
      Object.values(peerDependencyIssues.missingMergedByProjects)
        .every(({ conflicts, intersections }) => conflicts.length === 0 && Object.keys(intersections).length === 0)
    )
  ) return
  if (opts.strictPeerDependencies) {
    throw new PeerDependencyIssuesError(peerDependencyIssues)
  }
  peerDependencyIssuesLogger.debug(peerDependencyIssues)
}

export class PeerDependencyIssuesError extends PnpmError {
  issues: PeerDependencyIssues
  constructor (issues: PeerDependencyIssues) {
    super('PEER_DEP_ISSUES', 'Unmet peer dependencies')
    this.issues = issues
  }
}
