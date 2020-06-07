import { Config } from '@pnpm/config'
import * as logs from '@pnpm/core-loggers'
import { LogLevel } from '@pnpm/logger'
import most = require('most')
import reportBigTarballsProgress from './reportBigTarballsProgress'
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
    fetchingProgress: most.Stream<logs.FetchingProgressLog>,
    progress: most.Stream<logs.ProgressLog>,
    stage: most.Stream<logs.StageLog>,
    deprecation: most.Stream<logs.DeprecationLog>,
    summary: most.Stream<logs.SummaryLog>,
    lifecycle: most.Stream<logs.LifecycleLog>,
    stats: most.Stream<logs.StatsLog>,
    installCheck: most.Stream<logs.InstallCheckLog>,
    registry: most.Stream<logs.RegistryLog>,
    root: most.Stream<logs.RootLog>,
    packageManifest: most.Stream<logs.PackageManifestLog>,
    requestRetry: most.Stream<logs.RequestRetryLog>,
    link: most.Stream<logs.LinkLog>,
    other: most.Stream<logs.Log>,
    hook: most.Stream<logs.HookLog>,
    scope: most.Stream<logs.ScopeLog>,
    skippedOptionalDependency: most.Stream<logs.SkippedOptionalDependencyLog>,
  },
  opts: {
    appendOnly?: boolean,
    cmd: string,
    isRecursive: boolean,
    logLevel?: LogLevel,
    pnpmConfig?: Config,
    streamLifecycleOutput?: boolean,
    throttleProgress?: number,
    width?: number,
  }
): Array<most.Stream<most.Stream<{msg: string}>>> {
  const width = opts.width ?? process.stdout.columns ?? 80
  const cwd = opts.pnpmConfig?.dir ?? process.cwd()

  const outputs: Array<most.Stream<most.Stream<{msg: string}>>> = [
    reportProgress(log$, {
      cwd,
      throttleProgress: opts.throttleProgress,
    }),
    reportLifecycleScripts(log$, {
      appendOnly: opts.appendOnly || opts.streamLifecycleOutput,
      cwd,
      width,
    }),
    reportDeprecations(log$.deprecation, { cwd, isRecursive: opts.isRecursive }),
    reportMisc(
      log$,
      {
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
