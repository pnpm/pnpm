import * as Rx from 'rxjs'
import { map, take } from 'rxjs/operators'

import { ScopeLog } from '@pnpm/types'

const COMMANDS_THAT_REPORT_SCOPE = new Set([
  'install',
  'link',
  'prune',
  'rebuild',
  'remove',
  'unlink',
  'update',
  'run',
  'test',
])

export function reportScope(
  scope$: Rx.Observable<ScopeLog>,
  opts: {
    isRecursive: boolean
    cmd: string
  }
) {
  if (!COMMANDS_THAT_REPORT_SCOPE.has(opts.cmd)) {
    return Rx.NEVER
  }
  return scope$.pipe(
    take(1),
    map((log) => {
      if (log.selected === 1) {
        return Rx.NEVER
      }
      let msg = 'Scope: '

      if (log.selected === log.total) {
        msg += `all ${log.total}`
      } else {
        msg += `${log.selected}`
        if (log.total) {
          msg += ` of ${log.total}`
        }
      }

      msg += log.workspacePrefix ? ' workspace projects' : ' projects';

      return Rx.of({ msg })
    })
  )
}
