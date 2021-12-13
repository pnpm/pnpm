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
    Object.values(peerDependencyIssues).every((peerIssuesOfProject) =>
      isEmpty(peerIssuesOfProject.bad) && (
        isEmpty(peerIssuesOfProject.missing) ||
        peerIssuesOfProject.conflicts.length === 0 && Object.keys(peerIssuesOfProject.intersections).length === 0
      ))
  ) return
  if (opts.strictPeerDependencies) {
    throw new PeerDependencyIssuesError(peerDependencyIssues)
  }
  peerDependencyIssuesLogger.debug({
    issuesByProjects: peerDependencyIssues,
  })
}

export class PeerDependencyIssuesError extends PnpmError {
  issuesByProjects: PeerDependencyIssues
  constructor (issues: PeerDependencyIssues) {
    super('PEER_DEP_ISSUES', 'Unmet peer dependencies')
    this.issuesByProjects = issues
  }
}
