import { LinkLog, Log, RegistryLog } from '@pnpm/core-loggers'
import most = require('most')
import os = require('os')
import reportError from '../reportError'
import formatWarn from './utils/formatWarn'
import { autozoom } from './utils/zooming'

export default (
  log$: {
    registry: most.Stream<RegistryLog>,
    other: most.Stream<Log>,
  },
  opts: {
    cwd: string,
    zoomOutCurrent: boolean,
  },
) => {
  return most.merge(log$.registry, log$.other)
    .filter((obj) => obj.level !== 'debug' && (obj.level !== 'info' || !obj['prefix'] || obj['prefix'] === opts.cwd))
    .map((obj) => {
      switch (obj.level) {
        case 'warn':
          return autozoom(opts.cwd, obj.prefix, formatWarn(obj.message), opts)
        case 'error':
          if (obj['message'] && obj['message']['prefix'] && obj['message']['prefix'] !== opts.cwd) {
            return `${obj['message']['prefix']}:` + os.EOL + reportError(obj)
          }
          return reportError(obj)
        default:
          return obj['message']
      }
    })
    .map((msg) => ({ msg }))
    .map(most.of)
}
