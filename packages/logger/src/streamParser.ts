import bole from 'bole'
import ndjson from 'ndjson'
import { LogBase } from './LogBase'

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
