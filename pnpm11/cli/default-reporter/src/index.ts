import type * as logs from '@pnpm/core-loggers'
import type { LogLevel, StreamParser } from '@pnpm/logger'
import * as Rx from 'rxjs'
import { map, mergeAll } from 'rxjs/operators'

import { renderLiveOutput } from './liveOutput.js'
import { createLogStreams } from './logStreams.js'
import { mergeOutputs } from './mergeOutputs.js'
import { reporterForClient } from './reporterForClient/index.js'
import type { FilterPkgsDiff } from './reporterForClient/reportSummary.js'
import { formatWarn } from './reporterForClient/utils/formatWarn.js'
import type { ReporterPnpmConfig } from './ReporterPnpmConfig.js'

export { formatWarn }

export function initDefaultReporter (
  opts: {
    useStderr?: boolean
    streamParser: StreamParser<logs.Log>
    reportingOptions?: ReportingOptions & {
      appendOnly?: boolean
      outputMaxWidth?: number
    }
    context: ReportingContext
    filterPkgsDiff?: FilterPkgsDiff
  }
): () => void {
  const proc = opts.context.process ?? process
  // At least one column: `columns - 2` is zero on a two-column terminal, and a
  // caller may pass zero outright. A zero width would make every wrap
  // calculation — the differ's and `renderedRows`' — meaningless.
  const outputMaxWidth = Math.max(1, opts.reportingOptions?.outputMaxWidth ?? (proc.stdout.columns && proc.stdout.columns - 2) ?? 80)
  const output$ = toOutput$({
    ...opts,
    reportingOptions: {
      ...opts.reportingOptions,
      outputMaxWidth,
    },
  })
  if (opts.reportingOptions?.appendOnly) {
    return printAppendOnlyOutput(output$, opts.useStderr)
  }
  return renderLiveOutput(output$, {
    stream: opts.useStderr ? proc.stderr : proc.stdout,
    outputMaxWidth,
    streamParser: opts.streamParser,
  })
}

interface ReportingOptions {
  logLevel?: LogLevel
  streamLifecycleOutput?: boolean
  aggregateOutput?: boolean
  throttleProgress?: number
  hideAddedPkgsProgress?: boolean
  hideProgressPrefix?: boolean
  hideLifecycleOutput?: boolean
  hideLifecyclePrefix?: boolean
  // This is used by Bit CLI
  approveBuildsInstructionText?: string
}

interface ReportingContext {
  argv: string[]
  config?: ReporterPnpmConfig
  env?: NodeJS.ProcessEnv
  process?: NodeJS.Process
}

function printAppendOnlyOutput (output$: Rx.Observable<string>, useStderr: boolean | undefined): () => void {
  const writeNext = useStderr
    ? console.error.bind(console)
    : console.log.bind(console)
  const subscription = output$
    .subscribe({
      complete () {},
      error: (err) => {
        console.error(err.message)
      },
      next: writeNext,
    })
  return () => {
    subscription.unsubscribe()
  }
}

export function toOutput$ (
  opts: {
    streamParser: StreamParser<logs.Log>
    reportingOptions?: ReportingOptions & {
      appendOnly?: boolean
      outputMaxWidth?: number
    }
    context: ReportingContext
    filterPkgsDiff?: FilterPkgsDiff
  }
): Rx.Observable<string> {
  opts = opts || {}
  const log$ = createLogStreams(opts.streamParser, opts.context.config)
  const cmd = opts.context.argv[0]
  const outputs: Array<Rx.Observable<Rx.Observable<{ msg: string }>>> = reporterForClient(
    log$,
    {
      appendOnly: opts.reportingOptions?.appendOnly,
      cmd,
      config: opts.context.config,
      env: opts.context.env ?? process.env,
      filterPkgsDiff: opts.filterPkgsDiff,
      process: opts.context.process ?? process,
      isRecursive: opts.context.config?.['recursive'] === true,
      logLevel: opts.reportingOptions?.logLevel,
      pnpmConfig: opts.context.config,
      streamLifecycleOutput: opts.reportingOptions?.streamLifecycleOutput,
      aggregateOutput: opts.reportingOptions?.aggregateOutput,
      throttleProgress: opts.reportingOptions?.throttleProgress,
      width: opts.reportingOptions?.outputMaxWidth,
      hideAddedPkgsProgress: opts.reportingOptions?.hideAddedPkgsProgress,
      hideProgressPrefix: opts.reportingOptions?.hideProgressPrefix ?? (cmd === 'dlx' || opts.context.config?.global === true),
      hideLifecycleOutput: opts.reportingOptions?.hideLifecycleOutput,
      hideLifecyclePrefix: opts.reportingOptions?.hideLifecyclePrefix,
      approveBuildsInstructionText: opts.reportingOptions?.approveBuildsInstructionText,
    }
  )

  if (opts.reportingOptions?.appendOnly) {
    return Rx.merge(...outputs)
      .pipe(
        map((log: Rx.Observable<{ msg: string }>) => log.pipe(map((msg) => msg.msg))),
        mergeAll()
      )
  }
  return mergeOutputs(outputs)
}
