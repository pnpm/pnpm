import {streamParser, writeToConsole} from '@pnpm/logger'
import defaultReporter from 'pnpm-default-reporter'
import silentReporter from './silentReporter'

export type ReporterType = 'default' | 'ndjson' | 'silent' | 'append-only'

export default (reporterType: ReporterType, cmd: string) => {
  switch (reporterType) {
    case 'default':
      defaultReporter(streamParser, {
        appendOnly: false,
        cmd,
        throttleProgress: 0,
      })
      return
    case 'append-only':
      defaultReporter(streamParser, {
        appendOnly: true,
        cmd,
        throttleProgress: 0,
      })
      return
    case 'ndjson':
      writeToConsole()
      return
    case 'silent':
      silentReporter(streamParser)
      return
  }
}
