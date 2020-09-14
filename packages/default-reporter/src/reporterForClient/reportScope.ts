import { ScopeLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { map, take } from 'rxjs/operators'

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

export default (
  scope$: Rx.Observable<ScopeLog>,
  opts: {
    isRecursive: boolean
    cmd: string
  }
) => {
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

      if (log.workspacePrefix) {
        msg += ' workspace projects'
      } else {
        msg += ' projects'
      }

      return Rx.of({ msg })
    })
  )
}
