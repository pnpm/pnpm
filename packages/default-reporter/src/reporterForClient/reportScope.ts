import { ScopeLog } from '@pnpm/core-loggers'
import most = require('most')

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
  scope$: most.Stream<ScopeLog>,
  opts: {
    isRecursive: boolean
    cmd: string
  }
) => {
  if (!COMMANDS_THAT_REPORT_SCOPE.has(opts.cmd)) {
    return most.never()
  }
  return scope$
    .take(1)
    .map((log) => {
      if (log.selected === 1 && typeof log.total !== 'number') {
        if (!log.workspacePrefix) return most.never()
        if (!opts.isRecursive) return most.of({ msg: 'Scope: current workspace package' })
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

      return most.of({ msg })
    })
}
