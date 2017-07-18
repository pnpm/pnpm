import defaultReporter from 'pnpm-default-reporter'
import silentReporter from './silentReporter'
import {streamParser, writeToConsole} from 'pnpm-logger'

export type ReporterType = 'default' | 'ndjson' | 'silent'

export default (reporterType: ReporterType) => {
  switch (reporterType) {
    case 'default':
      defaultReporter(streamParser)
      return
    case 'ndjson':
      writeToConsole()
      return
    case 'silent':
      silentReporter(streamParser)
      return
  }
}
