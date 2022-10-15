import { Config } from '@pnpm/config'
import { initDefaultReporter } from '@pnpm/default-reporter'
import { LogLevel, streamParser, writeToConsole } from '@pnpm/logger'
import { silentReporter } from './silentReporter'

export type ReporterType = 'default' | 'ndjson' | 'silent' | 'append-only'

export function initReporter (
  reporterType: ReporterType,
  opts: {
    cmd: string | null
    config: Config
  }
) {
  switch (reporterType) {
  case 'default':
    initDefaultReporter({
      useStderr: opts.config.useStderr,
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
    initDefaultReporter({
      useStderr: opts.config.useStderr,
      context: {
        argv: opts.cmd ? [opts.cmd] : [],
        config: opts.config,
      },
      reportingOptions: {
        appendOnly: true,
        aggregateOutput: opts.config.aggregateOutput,
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
