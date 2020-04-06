import { LinkLog, Log, RegistryLog } from '@pnpm/core-loggers'
import { LogLevel } from '@pnpm/logger'
import most = require('most')
import os = require('os')
import reportError from '../reportError'
import formatWarn from './utils/formatWarn'
import { autozoom } from './utils/zooming'

// tslint:disable:object-literal-sort-keys
const LOG_LEVEL_NUMBER: Record<LogLevel, number> = {
  error: 0,
  warn: 1,
  info: 2,
  debug: 3,
}
// tslint:enable:object-literal-sort-keys

export default (
  log$: {
    registry: most.Stream<RegistryLog>,
    other: most.Stream<Log>,
  },
  opts: {
    cwd: string,
    logLevel?: LogLevel,
    zoomOutCurrent: boolean,
  },
) => {
  const maxLogLevel = LOG_LEVEL_NUMBER[opts.logLevel ?? 'info'] ?? LOG_LEVEL_NUMBER['info']
  return most.merge(log$.registry, log$.other)
    .filter((obj) => LOG_LEVEL_NUMBER[obj.level] <= maxLogLevel &&
      (obj.level !== 'info' || !obj['prefix'] || obj['prefix'] === opts.cwd))
    .map((obj) => {
      switch (obj.level) {
        case 'warn':
          return autozoom(opts.cwd, obj.prefix, formatWarn(obj.message), opts)
        case 'error':
          if (obj['message']?.['prefix'] && obj['message']['prefix'] !== opts.cwd) {
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
