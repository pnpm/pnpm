import { type Config } from '@pnpm/config'
import type * as logs from '@pnpm/core-loggers'
import { type LogLevel } from '@pnpm/logger'
import type * as Rx from 'rxjs'
import { throttleTime } from 'rxjs/operators'
import { reportBigTarballProgress } from './reportBigTarballsProgress.js'
import { reportContext } from './reportContext.js'
import { reportExecutionTime } from './reportExecutionTime.js'
import { reportDeprecations } from './reportDeprecations.js'
import { reportHooks } from './reportHooks.js'
import { reportIgnoredBuilds } from './reportIgnoredBuilds.js'
import { reportInstallChecks } from './reportInstallChecks.js'
import { reportInstallingConfigDeps } from './reportInstallingConfigDeps.js'
import { reportLifecycleScripts } from './reportLifecycleScripts.js'
import { reportMisc, LOG_LEVEL_NUMBER } from './reportMisc.js'
import { reportPeerDependencyIssues } from './reportPeerDependencyIssues.js'
import { reportProgress } from './reportProgress.js'
import { reportRequestRetry } from './reportRequestRetry.js'
import { reportScope } from './reportScope.js'
import { reportSkippedOptionalDependencies } from './reportSkippedOptionalDependencies.js'
import { reportStats } from './reportStats.js'
import { reportSummary, type FilterPkgsDiff } from './reportSummary.js'
import { reportUpdateCheck } from './reportUpdateCheck.js'
import { reportFunding } from './reportFunding.js'

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
    ignoredScripts: Rx.Observable<logs.IgnoredScriptsLog>
    progress: Rx.Observable<logs.ProgressLog>
    stage: Rx.Observable<logs.StageLog>
    deprecation: Rx.Observable<logs.DeprecationLog>
    summary: Rx.Observable<logs.SummaryLog>
    lifecycle: Rx.Observable<logs.LifecycleLog>
    stats: Rx.Observable<logs.StatsLog>
    installCheck: Rx.Observable<logs.InstallCheckLog>
    installingConfigDeps: Rx.Observable<logs.InstallingConfigDepsLog>
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
    funding: Rx.Observable<logs.FundingLog>
  },
  opts: {
    appendOnly?: boolean
    cmd: string
    config?: Config
    env: NodeJS.ProcessEnv
    filterPkgsDiff?: FilterPkgsDiff
    process: NodeJS.Process
    isRecursive: boolean
    logLevel?: LogLevel
    pnpmConfig?: Config
    streamLifecycleOutput?: boolean
    aggregateOutput?: boolean
    throttleProgress?: number
    width?: number
    hideAddedPkgsProgress?: boolean
    hideProgressPrefix?: boolean
    hideLifecycleOutput?: boolean
    hideLifecyclePrefix?: boolean
    // This is used by Bit CLI
    approveBuildsInstructionText?: string
  }
): Array<Rx.Observable<Rx.Observable<{ msg: string }>>> {
  const width = opts.width ?? process.stdout.columns ?? 80
  const cwd = opts.pnpmConfig?.dir ?? process.cwd()
  const throttle = typeof opts.throttleProgress === 'number' && opts.throttleProgress > 0
    ? throttleTime(opts.throttleProgress, undefined, { leading: true, trailing: true })
    : undefined

  const outputs: Array<Rx.Observable<Rx.Observable<{ msg: string }>>> = [
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
  ]

  // logLevelNumber: 0123 = error warn info debug
  const logLevelNumber = LOG_LEVEL_NUMBER[opts.logLevel ?? 'info'] ?? LOG_LEVEL_NUMBER['info']
  const showInfo = logLevelNumber >= LOG_LEVEL_NUMBER.info

  if (logLevelNumber >= LOG_LEVEL_NUMBER.warn) {
    outputs.push(
      reportPeerDependencyIssues(log$),
      reportDeprecations({
        deprecation: log$.deprecation,
        stage: log$.stage,
      }, { cwd, isRecursive: opts.isRecursive }),
      reportRequestRetry(log$.requestRetry)
    )
  }

  if (showInfo) {
    if (opts.cmd in PRINT_EXECUTION_TIME_IN_COMMANDS) {
      outputs.push(reportExecutionTime(log$.executionTime))
    }
    if (opts.cmd !== 'dlx') {
      outputs.push(reportContext(log$, { cwd }))
    }
    outputs.push(
      reportLifecycleScripts(log$, {
        appendOnly: (opts.appendOnly === true || opts.streamLifecycleOutput) && !opts.hideLifecycleOutput,
        aggregateOutput: opts.aggregateOutput,
        hideLifecyclePrefix: opts.hideLifecyclePrefix,
        cwd,
        width,
      }),
      reportInstallChecks(log$.installCheck, { cwd }),
      reportInstallingConfigDeps(log$.installingConfigDeps),
      reportScope(log$.scope, { isRecursive: opts.isRecursive, cmd: opts.cmd }),
      reportSkippedOptionalDependencies(log$.skippedOptionalDependency, { cwd }),
      reportHooks(log$.hook, { cwd, isRecursive: opts.isRecursive }),
      reportUpdateCheck(log$.updateCheck, opts),
      reportProgress(log$, {
        cwd,
        throttle,
        hideAddedPkgsProgress: opts.hideAddedPkgsProgress,
        hideProgressPrefix: opts.hideProgressPrefix,
      }),
      ...reportStats(log$, {
        cmd: opts.cmd,
        cwd,
        isRecursive: opts.isRecursive,
        width,
        hideProgressPrefix: opts.hideProgressPrefix,
      })
    )
    if (!opts.appendOnly) {
      outputs.push(reportBigTarballProgress(log$))
    }
    if (!opts.isRecursive) {
      outputs.push(reportSummary(log$, {
        cmd: opts.cmd,
        cwd,
        env: opts.env,
        filterPkgsDiff: opts.filterPkgsDiff,
        pnpmConfig: opts.pnpmConfig,
      }))
    }
    outputs.push(reportFunding(log$.funding))
    outputs.push(
      reportIgnoredBuilds(log$, {
        pnpmConfig: opts.pnpmConfig,
        approveBuildsInstructionText: opts.approveBuildsInstructionText,
      })
    )
  }

  return outputs
}
