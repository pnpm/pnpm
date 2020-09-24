import { Config } from '@pnpm/config'
import defaultReporter from '@pnpm/default-reporter'
import { LogLevel, streamParser, writeToConsole } from '@pnpm/logger'
import silentReporter from './silentReporter'

export type ReporterType = 'default' | 'ndjson' | 'silent' | 'append-only'

export default (
  reporterType: ReporterType,
  opts: {
    cmd: string | null
    config: Config
  }
) => {
  switch (reporterType) {
  case 'default':
    defaultReporter({
      context: {
        argv: opts.cmd ? [opts.cmd] : [],
        config: opts.config,
      },
      reportingOptions: {
        appendOnly: false,
        logLevel: opts.config.loglevel as LogLevel,
        streamLifecycleOutput: opts.config.stream,
        throttleProgress: 200,
      },
      streamParser,
    })
    return
  case 'append-only':
    defaultReporter({
      context: {
        argv: opts.cmd ? [opts.cmd] : [],
        config: opts.config,
      },
      reportingOptions: {
        appendOnly: true,
        logLevel: opts.config.loglevel as LogLevel,
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
  }
}
