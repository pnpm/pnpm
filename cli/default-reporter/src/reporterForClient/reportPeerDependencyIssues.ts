import * as Rx from 'rxjs'

import { renderPeerIssues } from '@pnpm/render-peer-issues'
import type { PeerDependencyIssuesLog, PeerDependencyRules } from '@pnpm/types'

import { map, take } from 'rxjs/operators'
import { formatWarn } from './utils/formatWarn'

export function reportPeerDependencyIssues(
  log$: {
    peerDependencyIssues: Rx.Observable<PeerDependencyIssuesLog>
  },
  peerDependencyRules?: PeerDependencyRules | undefined
) {
  return log$.peerDependencyIssues.pipe(
    take(1),
    map((log): Rx.Observable<{
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
