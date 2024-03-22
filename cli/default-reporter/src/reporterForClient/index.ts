import type * as Rx from 'rxjs'
import { throttleTime } from 'rxjs/operators'

import type { LogLevel } from '@pnpm/logger'
import type { ContextLog, DeprecationLog, ExecutionTimeLog, FetchingProgressLog, HookLog, InstallCheckLog, LifecycleLog, LinkLog, Log, PackageImportMethodLog, PackageManifestLog, PeerDependencyIssuesLog, ProgressLog, RegistryLog, RequestRetryLog, RootLog, ScopeLog, SkippedOptionalDependencyLog, StageLog, StatsLog, SummaryLog, UpdateCheckLog, PeerDependencyRules, Config } from '@pnpm/types'

import { reportStats } from './reportStats.js'
import { reportScope } from './reportScope.js'
import { reportHooks } from './reportHooks.js'
import { reportContext } from './reportContext.js'
import { reportProgress } from './reportProgress.js'
import { reportUpdateCheck } from './reportUpdateCheck.js'
import { reportRequestRetry } from './reportRequestRetry.js'
import { reportDeprecations } from './reportDeprecations.js'
import { reportMisc, LOG_LEVEL_NUMBER } from './reportMisc.js'
import { reportExecutionTime } from './reportExecutionTime.js'
import { reportInstallChecks } from './reportInstallChecks.js'
import { reportLifecycleScripts } from './reportLifecycleScripts.js'
import { reportSummary, type FilterPkgsDiff } from './reportSummary.js'
import { reportBigTarballProgress } from './reportBigTarballsProgress.js'
import { reportPeerDependencyIssues } from './reportPeerDependencyIssues.js'
import { reportSkippedOptionalDependencies } from './reportSkippedOptionalDependencies.js'

const PRINT_EXECUTION_TIME_IN_COMMANDS = {
  install: true,
  update: true,
  add: true,
  remove: true,
}

export function reporterForClient(
  log$: {
    context: Rx.Observable<ContextLog>
    fetchingProgress: Rx.Observable<FetchingProgressLog>
    executionTime: Rx.Observable<ExecutionTimeLog>
    progress: Rx.Observable<ProgressLog>
    stage: Rx.Observable<StageLog>
    deprecation: Rx.Observable<DeprecationLog>
    summary: Rx.Observable<SummaryLog>
    lifecycle: Rx.Observable<LifecycleLog>
    stats: Rx.Observable<StatsLog>
    installCheck: Rx.Observable<InstallCheckLog>
    registry: Rx.Observable<RegistryLog>
    root: Rx.Observable<RootLog>
    packageManifest: Rx.Observable<PackageManifestLog>
    peerDependencyIssues: Rx.Observable<PeerDependencyIssuesLog>
    requestRetry: Rx.Observable<RequestRetryLog>
    link: Rx.Observable<LinkLog>
    other: Rx.Observable<Log>
    hook: Rx.Observable<HookLog>
    scope: Rx.Observable<ScopeLog>
    skippedOptionalDependency: Rx.Observable<SkippedOptionalDependencyLog>
    packageImportMethod: Rx.Observable<PackageImportMethodLog>
    updateCheck: Rx.Observable<UpdateCheckLog>
  },
  opts: {
    appendOnly?: boolean | undefined
    cmd?: string | undefined
    config?: Config | undefined
    env: NodeJS.ProcessEnv
    filterPkgsDiff?: FilterPkgsDiff | undefined
    peerDependencyRules?: PeerDependencyRules | undefined
    process: NodeJS.Process
    isRecursive: boolean
    logLevel?: LogLevel | undefined
    pnpmConfig?: Config | undefined
    streamLifecycleOutput?: boolean | undefined
    aggregateOutput?: boolean | undefined
    throttleProgress?: number | undefined
    width?: number | undefined
    hideAddedPkgsProgress?: boolean | undefined
    hideProgressPrefix?: boolean | undefined
    hideLifecycleOutput?: boolean | undefined
    hideLifecyclePrefix?: boolean | undefined
  }
): Array<Rx.Observable<Rx.Observable<{ msg: string }>>> {
  const width = opts.width ?? process.stdout.columns ?? 80

  const cwd = opts.pnpmConfig?.dir ?? process.cwd()

  const throttle =
    typeof opts.throttleProgress === 'number' && opts.throttleProgress > 0
      ? throttleTime(opts.throttleProgress, undefined, {
        leading: true,
        trailing: true,
      })
      : undefined

  const outputs: Array<Rx.Observable<Rx.Observable<{ msg: string }>>> = [
    reportLifecycleScripts(log$, {
      appendOnly:
        (opts.appendOnly === true || opts.streamLifecycleOutput) &&
        !opts.hideLifecycleOutput,
      aggregateOutput: opts.aggregateOutput,
      hideLifecyclePrefix: opts.hideLifecyclePrefix,
      cwd,
      width,
    }),
    reportMisc(log$, {
      appendOnly: opts.appendOnly === true,
      config: opts.config,
      cwd,
      logLevel: opts.logLevel,
      zoomOutCurrent: opts.isRecursive,
      peerDependencyRules: opts.peerDependencyRules,
    }),
    reportInstallChecks(log$.installCheck, { cwd }),
    reportScope(log$.scope, { isRecursive: opts.isRecursive, cmd: opts.cmd }),
    reportSkippedOptionalDependencies(log$.skippedOptionalDependency, { cwd }),
    reportHooks(log$.hook, { cwd, isRecursive: opts.isRecursive }),
    reportUpdateCheck(log$.updateCheck, opts),
  ]

  if (opts.cmd !== 'dlx') {
    outputs.push(reportContext(log$, { cwd }))
  }

  if (typeof opts.cmd !== 'undefined' && opts.cmd in PRINT_EXECUTION_TIME_IN_COMMANDS) {
    outputs.push(reportExecutionTime(log$.executionTime))
  }

  // logLevelNumber: 0123 = error warn info debug
  const logLevelNumber =
    LOG_LEVEL_NUMBER[opts.logLevel ?? 'info'] ?? LOG_LEVEL_NUMBER.info

  if (logLevelNumber >= LOG_LEVEL_NUMBER.warn) {
    outputs.push(
      reportPeerDependencyIssues(log$, opts.peerDependencyRules),
      reportDeprecations(
        {
          deprecation: log$.deprecation,
          stage: log$.stage,
        },
        { cwd, isRecursive: opts.isRecursive }
      ),
      reportRequestRetry(log$.requestRetry)
    )
  }

  if (logLevelNumber >= LOG_LEVEL_NUMBER.info) {
    outputs.push(
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
  }

  if (!opts.appendOnly) {
    outputs.push(reportBigTarballProgress(log$))
  }

  if (!opts.isRecursive) {
    outputs.push(
      reportSummary(log$, {
        cmd: opts.cmd,
        cwd,
        env: opts.env,
        filterPkgsDiff: opts.filterPkgsDiff,
        pnpmConfig: opts.pnpmConfig,
      })
    )
  }

  return outputs
}
