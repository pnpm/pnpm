import { PeerDependencyIssuesLog } from '@pnpm/core-loggers'
import { renderPeerIssues } from '@pnpm/render-peer-issues'
import * as Rx from 'rxjs'
import { map, take } from 'rxjs/operators'
import formatWarn from './utils/formatWarn'

export default (
  log$: {
    peerDependencyIssues: Rx.Observable<PeerDependencyIssuesLog>
  }
) => {
  return log$.peerDependencyIssues.pipe(
    take(1),
    map((log) => Rx.of({
      msg: `${formatWarn('Issues with peer dependencies found')}\n${renderPeerIssues(log.issuesByProjects)}`,
    }))
  )
}
