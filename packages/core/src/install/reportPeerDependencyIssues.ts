import PnpmError from '@pnpm/error'
import { peerDependencyIssuesLogger } from '@pnpm/core-loggers'
import { PeerDependencyIssuesByProjects } from '@pnpm/types'
import isEmpty from 'ramda/src/isEmpty'

export default function (
  peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects,
  opts: {
    lockfileDir: string
    strictPeerDependencies: boolean
  }
) {
  if (
    Object.values(peerDependencyIssuesByProjects).every((peerIssuesOfProject) =>
      isEmpty(peerIssuesOfProject.bad) && (
        isEmpty(peerIssuesOfProject.missing) ||
        peerIssuesOfProject.conflicts.length === 0 && Object.keys(peerIssuesOfProject.intersections).length === 0
      ))
  ) return
  if (opts.strictPeerDependencies) {
    throw new PeerDependencyIssuesError(peerDependencyIssuesByProjects)
  }
  peerDependencyIssuesLogger.debug({
    issuesByProjects: peerDependencyIssuesByProjects,
  })
}

export class PeerDependencyIssuesError extends PnpmError {
  issuesByProjects: PeerDependencyIssuesByProjects
  constructor (issues: PeerDependencyIssuesByProjects) {
    super('PEER_DEP_ISSUES', 'Unmet peer dependencies')
    this.issuesByProjects = issues
  }
}
