import {streamParser, writeToConsole} from '@pnpm/logger'
import defaultReporter from 'pnpm-default-reporter'
import silentReporter from './silentReporter'

export type ReporterType = 'default' | 'ndjson' | 'silent' | 'append-only'

export default (reporterType: ReporterType, cmd: string) => {
  switch (reporterType) {
    case 'default':
      defaultReporter(streamParser, {
        cmd,
        appendOnly: false,
      })
      return
    case 'append-only':
      defaultReporter(streamParser, {
        cmd,
        appendOnly: true,
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
