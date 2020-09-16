import { Config } from '@pnpm/config'
import * as logs from '@pnpm/core-loggers'
import { LogLevel } from '@pnpm/logger'
import * as Rx from 'rxjs'
import { throttleTime } from 'rxjs/operators'
import reportBigTarballsProgress from './reportBigTarballsProgress'
import reportContext from './reportContext'
import reportDeprecations from './reportDeprecations'
import reportHooks from './reportHooks'
import reportInstallChecks from './reportInstallChecks'
import reportLifecycleScripts from './reportLifecycleScripts'
import reportMisc from './reportMisc'
import reportProgress from './reportProgress'
import reportRequestRetry from './reportRequestRetry'
import reportScope from './reportScope'
import reportSkippedOptionalDependencies from './reportSkippedOptionalDependencies'
import reportStats from './reportStats'
import reportSummary from './reportSummary'

export default function (
  log$: {
    context: Rx.Observable<logs.ContextLog>
    fetchingProgress: Rx.Observable<logs.FetchingProgressLog>
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
    requestRetry: Rx.Observable<logs.RequestRetryLog>
    link: Rx.Observable<logs.LinkLog>
    other: Rx.Observable<logs.Log>
    hook: Rx.Observable<logs.HookLog>
    scope: Rx.Observable<logs.ScopeLog>
    skippedOptionalDependency: Rx.Observable<logs.SkippedOptionalDependencyLog>
    packageImportMethod: Rx.Observable<logs.PackageImportMethodLog>
  },
  opts: {
    appendOnly?: boolean
    cmd: string
    config?: Config
    isRecursive: boolean
    logLevel?: LogLevel
    pnpmConfig?: Config
    streamLifecycleOutput?: boolean
    throttleProgress?: number
    width?: number
  }
): Array<Rx.Observable<Rx.Observable<{msg: string}>>> {
  const width = opts.width ?? process.stdout.columns ?? 80
  const cwd = opts.pnpmConfig?.dir ?? process.cwd()
  const throttle = typeof opts.throttleProgress === 'number' && opts.throttleProgress > 0
    ? throttleTime(opts.throttleProgress, undefined, { leading: true, trailing: true })
    : undefined

  const outputs: Array<Rx.Observable<Rx.Observable<{msg: string}>>> = [
    reportProgress(log$, {
      cwd,
      throttle,
    }),
    reportLifecycleScripts(log$, {
      appendOnly: opts.appendOnly === true || opts.streamLifecycleOutput,
      cwd,
      width,
    }),
    reportDeprecations(log$.deprecation, { cwd, isRecursive: opts.isRecursive }),
    reportMisc(
      log$,
      {
        config: opts.config,
        cwd,
        logLevel: opts.logLevel,
        zoomOutCurrent: opts.isRecursive,
      }
    ),
    ...reportStats(log$, {
      cmd: opts.cmd,
      cwd,
      isRecursive: opts.isRecursive,
      width,
    }),
    reportInstallChecks(log$.installCheck, { cwd }),
    reportRequestRetry(log$.requestRetry),
    reportScope(log$.scope, { isRecursive: opts.isRecursive, cmd: opts.cmd }),
    reportSkippedOptionalDependencies(log$.skippedOptionalDependency, { cwd }),
    reportHooks(log$.hook, { cwd, isRecursive: opts.isRecursive }),
    reportContext(log$, { cwd }),
  ]

  if (!opts.appendOnly) {
    outputs.push(reportBigTarballsProgress(log$))
  }

  if (!opts.isRecursive) {
    outputs.push(reportSummary(log$, {
      cwd,
      pnpmConfig: opts.pnpmConfig,
    }))
  }

  return outputs
}
