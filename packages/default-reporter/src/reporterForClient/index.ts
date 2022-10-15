import { Config } from '@pnpm/config'
import * as logs from '@pnpm/core-loggers'
import { LogLevel } from '@pnpm/logger'
import * as Rx from 'rxjs'
import { throttleTime } from 'rxjs/operators'
import { reportBigTarballProgress } from './reportBigTarballsProgress'
import { reportContext } from './reportContext'
import { reportExecutionTime } from './reportExecutionTime'
import { reportDeprecations } from './reportDeprecations'
import { reportHooks } from './reportHooks'
import { reportInstallChecks } from './reportInstallChecks'
import { reportLifecycleScripts } from './reportLifecycleScripts'
import { reportMisc, LOG_LEVEL_NUMBER } from './reportMisc'
import { reportPeerDependencyIssues } from './reportPeerDependencyIssues'
import { reportProgress } from './reportProgress'
import { reportRequestRetry } from './reportRequestRetry'
import { reportScope } from './reportScope'
import { reportSkippedOptionalDependencies } from './reportSkippedOptionalDependencies'
import { reportStats } from './reportStats'
import { reportSummary } from './reportSummary'
import { reportUpdateCheck } from './reportUpdateCheck'

const PRINT_EXECUTION_TIME_IN_COMMANDS = {
  install: true,
  update: true,
  add: true,
  remove: true,
}

export function reporterForClient (
  log$: {
    context: Rx.Observable<logs.ContextLog>
    fetchingProgress: Rx.Observable<logs.FetchingProgressLog>
    executionTime: Rx.Observable<logs.ExecutionTimeLog>
    progress: Rx.Observable<logs.ProgressLog>
    stage: Rx.Observable<logs.StageLog>
    deprecation: Rx.Observable<logs.DeprecationLog>
    summary: Rx.Observable<logs.SummaryLog>
    lifecycle: Rx.Observable<logs.LifecycleLog>
    stats: Rx.Observable<logs.StatsLog>
    installCheck: Rx.Observable<logs.InstallCheckLog>
    registry: Rx.Observable<logs.RegistryLog>
    root: Rx.Observable<logs.RootLog>
    packageManifest: Rx.Observable<logs.PackageManifestLog>
    peerDependencyIssues: Rx.Observable<logs.PeerDependencyIssuesLog>
    requestRetry: Rx.Observable<logs.RequestRetryLog>
    link: Rx.Observable<logs.LinkLog>
    other: Rx.Observable<logs.Log>
    hook: Rx.Observable<logs.HookLog>
    scope: Rx.Observable<logs.ScopeLog>
    skippedOptionalDependency: Rx.Observable<logs.SkippedOptionalDependencyLog>
    packageImportMethod: Rx.Observable<logs.PackageImportMethodLog>
    updateCheck: Rx.Observable<logs.UpdateCheckLog>
  },
  opts: {
    appendOnly?: boolean
    cmd: string
    config?: Config
    env: NodeJS.ProcessEnv
    isRecursive: boolean
    logLevel?: LogLevel
    pnpmConfig?: Config
    streamLifecycleOutput?: boolean
    aggregateOutput?: boolean
    throttleProgress?: number
    width?: number
  }
): Array<Rx.Observable<Rx.Observable<{ msg: string }>>> {
  const width = opts.width ?? process.stdout.columns ?? 80
  const cwd = opts.pnpmConfig?.dir ?? process.cwd()
  const throttle = typeof opts.throttleProgress === 'number' && opts.throttleProgress > 0
    ? throttleTime(opts.throttleProgress, undefined, { leading: true, trailing: true })
    : undefined

  const outputs: Array<Rx.Observable<Rx.Observable<{ msg: string }>>> = [
    reportLifecycleScripts(log$, {
      appendOnly: opts.appendOnly === true || opts.streamLifecycleOutput,
      aggregateOutput: opts.aggregateOutput,
      cwd,
      width,
    }),
    reportMisc(
      log$,
      {
        appendOnly: opts.appendOnly === true,
        config: opts.config,
        cwd,
        logLevel: opts.logLevel,
        zoomOutCurrent: opts.isRecursive,
      }
    ),
    reportInstallChecks(log$.installCheck, { cwd }),
    reportScope(log$.scope, { isRecursive: opts.isRecursive, cmd: opts.cmd }),
    reportSkippedOptionalDependencies(log$.skippedOptionalDependency, { cwd }),
    reportHooks(log$.hook, { cwd, isRecursive: opts.isRecursive }),
    reportContext(log$, { cwd }),
    reportUpdateCheck(log$.updateCheck, opts),
  ]

  if (PRINT_EXECUTION_TIME_IN_COMMANDS[opts.cmd]) {
    outputs.push(reportExecutionTime(log$.executionTime))
  }

  // logLevelNumber: 0123 = error warn info debug
  const logLevelNumber = LOG_LEVEL_NUMBER[opts.logLevel ?? 'info'] ?? LOG_LEVEL_NUMBER['info']

  if (logLevelNumber >= LOG_LEVEL_NUMBER.warn) {
    outputs.push(
      reportPeerDependencyIssues(log$),
      reportDeprecations(log$.deprecation, { cwd, isRecursive: opts.isRecursive }),
      reportRequestRetry(log$.requestRetry)
    )
  }

  if (logLevelNumber >= LOG_LEVEL_NUMBER.info) {
    outputs.push(
      reportProgress(log$, {
        cwd,
        throttle,
      }),
      ...reportStats(log$, {
        cmd: opts.cmd,
        cwd,
        isRecursive: opts.isRecursive,
        width,
      })
    )
  }

  if (!opts.appendOnly) {
    outputs.push(reportBigTarballProgress(log$))
  }

  if (!opts.isRecursive) {
    outputs.push(reportSummary(log$, {
      cwd,
      env: opts.env,
      pnpmConfig: opts.pnpmConfig,
    }))
  }

  return outputs
}
