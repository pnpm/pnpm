import bole from 'bole'
import ndjson from 'ndjson'

export type LogLevel = 'error' | 'warn' | 'info' | 'debug'

export interface LogBaseTemplate {
  level?: LogLevel
  prefix?: string
  message?: string
}

export interface LogBaseDebug extends LogBaseTemplate {
  level: 'debug'
}

export interface LogBaseError extends LogBaseTemplate {
  level: 'error'
}

export interface LogBaseInfo extends LogBaseTemplate {
  level: 'info'
  prefix: string
  message: string
}

export interface LogBaseWarn extends LogBaseTemplate {
  level: 'warn'
  prefix: string
  message: string
}

export type LogBase = LogBaseDebug | LogBaseError | LogBaseInfo | LogBaseWarn

export type Reporter = (logObj: LogBase) => void

export interface StreamParser {
  on: (event: 'data', reporter: Reporter) => void
  removeListener: (event: 'data', reporter: Reporter) => void
}

export const streamParser: StreamParser = createStreamParser()

export function createStreamParser (): StreamParser {
  const sp: StreamParser = ndjson.parse()
  bole.output([
    {
      level: 'debug', stream: sp,
    },
  ])
  return sp
}
