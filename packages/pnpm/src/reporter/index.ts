import {streamParser, writeToConsole} from '@pnpm/logger'
import defaultReporter from 'pnpm-default-reporter'
import silentReporter from './silentReporter'

export type ReporterType = 'default' | 'ndjson' | 'silent' | 'append-only'

export default (reporterType: ReporterType, cmd: string, subCmd: string) => {
  switch (reporterType) {
    case 'default':
      defaultReporter(streamParser, {
        appendOnly: false,
        cmd,
        subCmd,
        throttleProgress: 200,
      })
      return
    case 'append-only':
      defaultReporter(streamParser, {
        appendOnly: true,
        cmd,
        subCmd,
        throttleProgress: 1000,
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
