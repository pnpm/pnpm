import { type PeerDependencyIssuesLog } from '@pnpm/core-loggers'
import { renderPeerIssues } from '@pnpm/render-peer-issues'
import { type PeerDependencyRules } from '@pnpm/types'
import * as Rx from 'rxjs'
import { map, take } from 'rxjs/operators'
import { formatWarn } from './utils/formatWarn'

export function reportPeerDependencyIssues (
  log$: {
    peerDependencyIssues: Rx.Observable<PeerDependencyIssuesLog>
  },
  peerDependencyRules?: PeerDependencyRules
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  return log$.peerDependencyIssues.pipe(
    take(1),
    map((log) => {
      const renderedPeerIssues = renderPeerIssues(log.issuesByProjects, {
        rules: peerDependencyRules,
      })
      if (!renderedPeerIssues) {
        return Rx.NEVER
      }
      return Rx.of({
        msg: `${formatWarn('Issues with peer dependencies found')}\n${renderedPeerIssues}`,
      })
    })
  )
}
