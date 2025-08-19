import { type PeerDependencyIssuesLog } from '@pnpm/core-loggers'
import { renderPeerIssues } from '@pnpm/render-peer-issues'
import * as Rx from 'rxjs'
import { map, take } from 'rxjs/operators'
import { formatWarn } from './utils/formatWarn.js'

export function reportPeerDependencyIssues (
  log$: {
    peerDependencyIssues: Rx.Observable<PeerDependencyIssuesLog>
  }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  return log$.peerDependencyIssues.pipe(
    take(1),
    map((log) => {
      const renderedPeerIssues = renderPeerIssues(log.issuesByProjects)
      if (!renderedPeerIssues) {
        return Rx.NEVER
      }
      return Rx.of({
        msg: `${formatWarn('Issues with peer dependencies found')}\n${renderedPeerIssues}`,
      })
    })
  )
}
