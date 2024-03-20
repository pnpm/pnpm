import type { Config, ReporterType } from '@pnpm/types'
import { initDefaultReporter } from '@pnpm/default-reporter'
import { type LogLevel, streamParser, writeToConsole } from '@pnpm/logger'

import { silentReporter } from './silentReporter'

export function initReporter(
  reporterType: ReporterType,
  opts: {
    cmd: string | null
    config: Config
  }
): void {
  switch (reporterType) {
    case 'default': {
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
          hideAddedPkgsProgress: opts.config.lockfileOnly,
          hideLifecyclePrefix: opts.config.reporterHidePrefix,
        },
        streamParser,
      })

      return
    }

    case 'append-only': {
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
          hideLifecyclePrefix: opts.config.reporterHidePrefix,
        },
        streamParser,
      })

      return
    }

    case 'ndjson': {
      writeToConsole()

      return
    }

    case 'silent': {
      silentReporter(streamParser)
    }
  }
}
