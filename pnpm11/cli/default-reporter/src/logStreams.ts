import type * as logs from '@pnpm/core-loggers'
import type { StreamParser } from '@pnpm/logger'
import * as Rx from 'rxjs'
import { filter } from 'rxjs/operators'

import type { ReporterPnpmConfig } from './ReporterPnpmConfig.js'

interface LogsByStream {
  context: logs.ContextLog
  deprecation: logs.DeprecationLog
  fetchingProgress: logs.FetchingProgressLog
  executionTime: logs.ExecutionTimeLog
  hook: logs.HookLog
  installCheck: logs.InstallCheckLog
  installingConfigDeps: logs.InstallingConfigDepsLog
  ignoredScripts: logs.IgnoredScriptsLog
  lifecycle: logs.LifecycleLog
  link: logs.LinkLog
  lockfileVerification: logs.LockfileVerificationLog
  other: logs.Log
  packageImportMethod: logs.PackageImportMethodLog
  packageManifest: logs.PackageManifestLog
  peerDependencyIssues: logs.PeerDependencyIssuesLog
  progress: logs.ProgressLog
  registry: logs.RegistryLog
  requestRetry: logs.RequestRetryLog
  root: logs.RootLog
  scope: logs.ScopeLog
  skippedOptionalDependency: logs.SkippedOptionalDependencyLog
  stage: logs.StageLog
  stats: logs.StatsLog
  summary: logs.SummaryLog
  updateCheck: logs.UpdateCheckLog
}

type LogStreamName = keyof LogsByStream

export type LogStreams = { [Name in LogStreamName]: Rx.Observable<LogsByStream[Name]> }

type LogSubjects = { [Name in LogStreamName]: Rx.Subject<LogsByStream[Name]> }

// `pnpm:prompt` logs are not routed anywhere: prompts only pause redraws,
// which the live renderer handles.
const LOG_STREAM_BY_LOG_NAME = new Map<string, LogStreamName>([
  ['pnpm:context', 'context'],
  ['pnpm:execution-time', 'executionTime'],
  ['pnpm:fetching-progress', 'fetchingProgress'],
  ['pnpm:progress', 'progress'],
  ['pnpm:stage', 'stage'],
  ['pnpm:deprecation', 'deprecation'],
  ['pnpm:summary', 'summary'],
  ['pnpm:lifecycle', 'lifecycle'],
  ['pnpm:lockfile-verification', 'lockfileVerification'],
  ['pnpm:stats', 'stats'],
  ['pnpm:package-import-method', 'packageImportMethod'],
  ['pnpm:peer-dependency-issues', 'peerDependencyIssues'],
  ['pnpm:install-check', 'installCheck'],
  ['pnpm:installing-config-deps', 'installingConfigDeps'],
  ['pnpm:ignored-scripts', 'ignoredScripts'],
  ['pnpm:registry', 'registry'],
  ['pnpm:root', 'root'],
  ['pnpm:package-manifest', 'packageManifest'],
  ['pnpm:link', 'link'],
  ['pnpm:hook', 'hook'],
  ['pnpm:skipped-optional-dependency', 'skippedOptionalDependency'],
  ['pnpm:scope', 'scope'],
  ['pnpm:request-retry', 'requestRetry'],
  ['pnpm:update-check', 'updateCheck'],
  ['pnpm', 'other'],
  ['pnpm:global', 'other'],
  ['pnpm:store', 'other'],
  ['pnpm:lockfile', 'other'],
])

/**
 * Splits the logs of `streamParser` into one stream per log type. The
 * subscription to `streamParser` starts on the next tick.
 */
export function createLogStreams (
  streamParser: StreamParser<logs.Log>,
  config: ReporterPnpmConfig | undefined
): LogStreams {
  const subjects = createLogSubjects()
  setTimeout(() => {
    streamParser.on('data', (log: logs.Log) => {
      const streamName = LOG_STREAM_BY_LOG_NAME.get(log.name)
      if (streamName == null) return
      (subjects[streamName] as Rx.Subject<logs.Log>).next(log)
    })
  }, 0)
  return {
    ...observeLogSubjects(subjects),
    other: filterOtherLogs(Rx.from(subjects.other), config),
  }
}

function filterOtherLogs (other: Rx.Observable<logs.Log>, config: ReporterPnpmConfig | undefined): Rx.Observable<logs.Log> {
  if (config?.hooks?.filterLog == null) return other
  const filterLogs = config.hooks.filterLog
  const filterFn = filterLogs.length === 1
    ? filterLogs[0]
    : (log: logs.Log) => filterLogs.every!((filterLog) => filterLog(log))
  return other.pipe(filter(filterFn))
}

function createLogSubjects (): LogSubjects {
  return {
    context: new Rx.Subject(),
    deprecation: new Rx.Subject(),
    fetchingProgress: new Rx.Subject(),
    executionTime: new Rx.Subject(),
    hook: new Rx.Subject(),
    installCheck: new Rx.Subject(),
    installingConfigDeps: new Rx.Subject(),
    ignoredScripts: new Rx.Subject(),
    lifecycle: new Rx.Subject(),
    link: new Rx.Subject(),
    lockfileVerification: new Rx.Subject(),
    other: new Rx.Subject(),
    packageImportMethod: new Rx.Subject(),
    packageManifest: new Rx.Subject(),
    peerDependencyIssues: new Rx.Subject(),
    progress: new Rx.Subject(),
    registry: new Rx.Subject(),
    requestRetry: new Rx.Subject(),
    root: new Rx.Subject(),
    scope: new Rx.Subject(),
    skippedOptionalDependency: new Rx.Subject(),
    stage: new Rx.Subject(),
    stats: new Rx.Subject(),
    summary: new Rx.Subject(),
    updateCheck: new Rx.Subject(),
  }
}

function observeLogSubjects (subjects: LogSubjects): LogStreams {
  return {
    context: Rx.from(subjects.context),
    deprecation: Rx.from(subjects.deprecation),
    fetchingProgress: Rx.from(subjects.fetchingProgress),
    executionTime: Rx.from(subjects.executionTime),
    hook: Rx.from(subjects.hook),
    installCheck: Rx.from(subjects.installCheck),
    installingConfigDeps: Rx.from(subjects.installingConfigDeps),
    ignoredScripts: Rx.from(subjects.ignoredScripts),
    lifecycle: Rx.from(subjects.lifecycle),
    link: Rx.from(subjects.link),
    lockfileVerification: Rx.from(subjects.lockfileVerification),
    other: Rx.from(subjects.other),
    packageImportMethod: Rx.from(subjects.packageImportMethod),
    packageManifest: Rx.from(subjects.packageManifest),
    peerDependencyIssues: Rx.from(subjects.peerDependencyIssues),
    progress: Rx.from(subjects.progress),
    registry: Rx.from(subjects.registry),
    requestRetry: Rx.from(subjects.requestRetry),
    root: Rx.from(subjects.root),
    scope: Rx.from(subjects.scope),
    skippedOptionalDependency: Rx.from(subjects.skippedOptionalDependency),
    stage: Rx.from(subjects.stage),
    stats: Rx.from(subjects.stats),
    summary: Rx.from(subjects.summary),
    updateCheck: Rx.from(subjects.updateCheck),
  }
}
