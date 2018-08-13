import {PnpmConfigs} from '@pnpm/config'
import {streamParser, writeToConsole} from '@pnpm/logger'
import defaultReporter from 'pnpm-default-reporter'
import silentReporter from './silentReporter'

export type ReporterType = 'default' | 'ndjson' | 'silent' | 'append-only'

export default (
  reporterType: ReporterType,
  opts: {
    cmd: string,
    subCmd: string,
    pnpmConfigs: PnpmConfigs,
  },
) => {
  switch (reporterType) {
    case 'default':
      defaultReporter({
        context: {
          argv: [opts.cmd, opts.subCmd],
          configs: opts.pnpmConfigs,
        },
        reportingOptions: {
          appendOnly: false,
          throttleProgress: 200,
        },
        streamParser,
      })
      return
    case 'append-only':
      defaultReporter({
        context: {
          argv: [opts.cmd, opts.subCmd],
          configs: opts.pnpmConfigs,
        },
        reportingOptions: {
          appendOnly: true,
          throttleProgress: 1000,
        },
        streamParser,
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
