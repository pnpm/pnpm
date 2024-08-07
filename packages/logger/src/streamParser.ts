import bole from 'bole'
import ndjson from 'ndjson'
import { type LogBase } from './LogBase'

export type Reporter<LogObj extends LogBase> = (logObj: LogObj) => void

export interface StreamParser<LogObj extends LogBase> {
  on: (event: 'data', reporter: Reporter<LogObj>) => void
  removeListener: (event: 'data', reporter: Reporter<LogObj>) => void
}

export const streamParser: StreamParser<LogBase> = createStreamParser()

export function createStreamParser<LogObj extends LogBase> (): StreamParser<LogObj> {
  const sp: StreamParser<LogObj> = ndjson.parse()
  bole.output([
    {
      level: 'debug', stream: sp,
    },
  ])
  return sp
}
