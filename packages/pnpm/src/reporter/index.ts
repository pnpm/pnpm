import {streamParser, writeToConsole} from '@pnpm/logger'
import defaultReporter from 'pnpm-default-reporter'
import silentReporter from './silentReporter'

export type ReporterType = 'default' | 'ndjson' | 'silent' | 'append-only'

export default (
  reporterType: ReporterType,
  opts: {
    cmd: string,
    subCmd: string,
    prefix: string,
  },
) => {
  switch (reporterType) {
    case 'default':
      defaultReporter(streamParser, {
        appendOnly: false,
        cmd: opts.cmd,
        cwd: opts.prefix,
        subCmd: opts.subCmd,
        throttleProgress: 200,
      })
      return
    case 'append-only':
      defaultReporter(streamParser, {
        appendOnly: true,
        cmd: opts.cmd,
        cwd: opts.prefix,
        subCmd: opts.subCmd,
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
