import * as Rx from 'rxjs'

import { map, take } from 'rxjs/operators'

import { renderPeerIssues } from '@pnpm/render-peer-issues'
import type { PeerDependencyIssuesLog, PeerDependencyRules } from '@pnpm/types'

import { formatWarn } from './utils/formatWarn.js'

export function reportPeerDependencyIssues(
  log$: {
    peerDependencyIssues: Rx.Observable<PeerDependencyIssuesLog>
  },
  peerDependencyRules?: PeerDependencyRules | undefined
): Rx.Observable<Rx.Observable<{
    msg: string;
  }>> {
  return log$.peerDependencyIssues.pipe(
    take(1),
    map((log: PeerDependencyIssuesLog): Rx.Observable<{
      msg: string;
    }> => {
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
