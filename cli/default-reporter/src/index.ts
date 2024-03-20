import '@total-typescript/ts-reset'

import * as Rx from 'rxjs'
import { filter, map, mergeAll } from 'rxjs/operators'
import createDiffer from 'ansi-diff'
import { EOL } from './constants'
import { mergeOutputs } from './mergeOutputs'
import { reporterForClient } from './reporterForClient'
import { formatWarn } from './reporterForClient/utils/formatWarn'
import { reporterForServer } from './reporterForServer'
import type { FilterPkgsDiff } from './reporterForClient/reportSummary'
import type { Config, ContextLog, DeprecationLog, ExecutionTimeLog, FetchingProgressLog, HookLog, InstallCheckLog, LifecycleLog, LinkLog, Log, PackageImportMethodLog, PackageManifestLog, PeerDependencyIssuesLog, PeerDependencyRules, ProgressLog, RegistryLog, RequestRetryLog, RootLog, ScopeLog, SkippedOptionalDependencyLog, StageLog, StatsLog, SummaryLog, UpdateCheckLog } from '@pnpm/types'
import type { LogLevel } from '@pnpm/logger'
import { StreamParser } from '@pnpm/logger/lib/streamParser'

export { formatWarn }

export function initDefaultReporter(opts: {
  useStderr?: boolean | undefined
  streamParser: StreamParser
  reportingOptions?: {
    appendOnly?: boolean | undefined
    logLevel?: LogLevel | undefined
    streamLifecycleOutput?: boolean | undefined
    aggregateOutput?: boolean | undefined
    throttleProgress?: number | undefined
    outputMaxWidth?: number | undefined
    hideAddedPkgsProgress?: boolean | undefined
    hideProgressPrefix?: boolean | undefined
    hideLifecycleOutput?: boolean | undefined
    hideLifecyclePrefix?: boolean | undefined
    peerDependencyRules?: PeerDependencyRules | undefined
  } | undefined
  context: {
    argv: string[]
    config?: Config | undefined
    env?: NodeJS.ProcessEnv | undefined
    process?: NodeJS.Process | undefined
  }
  filterPkgsDiff?: FilterPkgsDiff | undefined
}): () => void {
  if (opts.context.argv[0] === 'server') {
    // eslint-disable-next-line
    const log$ = Rx.fromEvent<Log>(opts.streamParser as any, 'data')

    const subscription = reporterForServer(log$, opts.context.config)

    return () => {
      subscription.unsubscribe()
    }
  }

  const outputMaxWidth =
    opts.reportingOptions?.outputMaxWidth ??
    (process.stdout.columns && process.stdout.columns - 2) ??
    80

  const output$ = toOutput$({
    ...opts,
    reportingOptions: {
      ...opts.reportingOptions,
      outputMaxWidth,
    },
  })

  if (opts.reportingOptions?.appendOnly) {
    const writeNext = opts.useStderr
      ? console.error.bind(console)
      : console.log.bind(console)

    const subscription = output$.subscribe({
      complete() {}, // eslint-disable-line:no-empty
      error: (err) => {
        console.error(err.message)
      },
      next: writeNext,
    })

    return () => {
      subscription.unsubscribe()
    }
  }

  const diff = createDiffer({
    height: process.stdout.rows,
    outputMaxWidth,
  })

  const subscription = output$.subscribe({
    complete() {}, // eslint-disable-line:no-empty
    error: (err) => {
      logUpdate(err.message)
    },
    next: logUpdate,
  })

  const write = opts.useStderr
    ? process.stderr.write.bind(process.stderr)
    : process.stdout.write.bind(process.stdout)

  function logUpdate(view: string) {
    // A new line should always be appended in case a prompt needs to appear.
    // Without a new line the prompt will be joined with the previous output.
    // An example of such prompt may be seen by running: pnpm update --interactive
    if (!view.endsWith(EOL)) {
      view += EOL
    }

    write(diff.update(view))
  }

  return () => {
    subscription.unsubscribe()
  }
}

export function toOutput$(opts: {
  streamParser: StreamParser
  reportingOptions?: {
    appendOnly?: boolean
    logLevel?: LogLevel
    outputMaxWidth?: number
    peerDependencyRules?: PeerDependencyRules
    streamLifecycleOutput?: boolean
    aggregateOutput?: boolean
    throttleProgress?: number
    hideAddedPkgsProgress?: boolean
    hideProgressPrefix?: boolean
    hideLifecycleOutput?: boolean
    hideLifecyclePrefix?: boolean
  }
  context: {
    argv: string[]
    config?: Config
    env?: NodeJS.ProcessEnv
    process?: NodeJS.Process
  }
  filterPkgsDiff?: FilterPkgsDiff
}): Rx.Observable<string> {
  opts = opts || {}
  const contextPushStream = new Rx.Subject<ContextLog>()
  const fetchingProgressPushStream = new Rx.Subject<FetchingProgressLog>()
  const executionTimePushStream = new Rx.Subject<ExecutionTimeLog>()
  const progressPushStream = new Rx.Subject<ProgressLog>()
  const stagePushStream = new Rx.Subject<StageLog>()
  const deprecationPushStream = new Rx.Subject<DeprecationLog>()
  const summaryPushStream = new Rx.Subject<SummaryLog>()
  const lifecyclePushStream = new Rx.Subject<LifecycleLog>()
  const statsPushStream = new Rx.Subject<StatsLog>()
  const packageImportMethodPushStream =
    new Rx.Subject<PackageImportMethodLog>()
  const installCheckPushStream = new Rx.Subject<InstallCheckLog>()
  const registryPushStream = new Rx.Subject<RegistryLog>()
  const rootPushStream = new Rx.Subject<RootLog>()
  const packageManifestPushStream = new Rx.Subject<PackageManifestLog>()
  const peerDependencyIssuesPushStream =
    new Rx.Subject<PeerDependencyIssuesLog>()
  const linkPushStream = new Rx.Subject<LinkLog>()
  const otherPushStream = new Rx.Subject<Log>()
  const hookPushStream = new Rx.Subject<HookLog>()
  const skippedOptionalDependencyPushStream =
    new Rx.Subject<SkippedOptionalDependencyLog>()
  const scopePushStream = new Rx.Subject<ScopeLog>()
  const requestRetryPushStream = new Rx.Subject<RequestRetryLog>()
  const updateCheckPushStream = new Rx.Subject<UpdateCheckLog>()
  globalThis.setTimeout(() => {
    opts.streamParser.on('data', (log: Log) => {
      switch (log.name) {
        case 'pnpm:context':
          contextPushStream.next(log)
          break
        case 'pnpm:execution-time':
          executionTimePushStream.next(log)
          break
        case 'pnpm:fetching-progress':
          fetchingProgressPushStream.next(log)
          break
        case 'pnpm:progress':
          progressPushStream.next(log)
          break
        case 'pnpm:stage':
          stagePushStream.next(log)
          break
        case 'pnpm:deprecation':
          deprecationPushStream.next(log)
          break
        case 'pnpm:summary':
          summaryPushStream.next(log)
          break
        case 'pnpm:lifecycle':
          lifecyclePushStream.next(log)
          break
        case 'pnpm:stats':
          statsPushStream.next(log)
          break
        case 'pnpm:package-import-method':
          packageImportMethodPushStream.next(log)
          break
        case 'pnpm:peer-dependency-issues':
          peerDependencyIssuesPushStream.next(log)
          break
        case 'pnpm:install-check':
          installCheckPushStream.next(log)
          break
        case 'pnpm:registry':
          registryPushStream.next(log)
          break
        case 'pnpm:root':
          rootPushStream.next(log)
          break
        case 'pnpm:package-manifest':
          packageManifestPushStream.next(log)
          break
        case 'pnpm:link':
          linkPushStream.next(log)
          break
        case 'pnpm:hook':
          hookPushStream.next(log)
          break
        case 'pnpm:skipped-optional-dependency':
          skippedOptionalDependencyPushStream.next(log)
          break
        case 'pnpm:scope':
          scopePushStream.next(log)
          break
        case 'pnpm:request-retry':
          requestRetryPushStream.next(log)
          break
        case 'pnpm:update-check':
          updateCheckPushStream.next(log)
          break
      case 'pnpm' as any: // eslint-disable-line
      case 'pnpm:global' as any: // eslint-disable-line
      case 'pnpm:store' as any: // eslint-disable-line
      case 'pnpm:lockfile' as any: // eslint-disable-line
          otherPushStream.next(log)
          break
      }
    })
  }, 0)
  let other = Rx.from(otherPushStream)
  if (opts.context.config?.hooks?.filterLog != null) {
    const filterLogs = opts.context.config.hooks.filterLog
    const filterFn =
      filterLogs.length === 1
        ? filterLogs[0]
        : (log: Log) => filterLogs.every!((filterLog) => filterLog(log))
    other = other.pipe(filter(filterFn))
  }
  const log$ = {
    context: Rx.from(contextPushStream),
    deprecation: Rx.from(deprecationPushStream),
    fetchingProgress: Rx.from(fetchingProgressPushStream),
    executionTime: Rx.from(executionTimePushStream),
    hook: Rx.from(hookPushStream),
    installCheck: Rx.from(installCheckPushStream),
    lifecycle: Rx.from(lifecyclePushStream),
    link: Rx.from(linkPushStream),
    other,
    packageImportMethod: Rx.from(packageImportMethodPushStream),
    packageManifest: Rx.from(packageManifestPushStream),
    peerDependencyIssues: Rx.from(peerDependencyIssuesPushStream),
    progress: Rx.from(progressPushStream),
    registry: Rx.from(registryPushStream),
    requestRetry: Rx.from(requestRetryPushStream),
    root: Rx.from(rootPushStream),
    scope: Rx.from(scopePushStream),
    skippedOptionalDependency: Rx.from(skippedOptionalDependencyPushStream),
    stage: Rx.from(stagePushStream),
    stats: Rx.from(statsPushStream),
    summary: Rx.from(summaryPushStream),
    updateCheck: Rx.from(updateCheckPushStream),
  }
  const cmd = opts.context.argv[0]
  const outputs: Array<Rx.Observable<Rx.Observable<{ msg: string }>>> =
    reporterForClient(log$, {
      appendOnly: opts.reportingOptions?.appendOnly,
      cmd,
      config: opts.context.config,
      env: opts.context.env ?? process.env,
      filterPkgsDiff: opts.filterPkgsDiff,
      peerDependencyRules: opts.reportingOptions?.peerDependencyRules,
      process: opts.context.process ?? process,
      isRecursive: opts.context.config?.recursive === true,
      logLevel: opts.reportingOptions?.logLevel,
      pnpmConfig: opts.context.config,
      streamLifecycleOutput: opts.reportingOptions?.streamLifecycleOutput,
      aggregateOutput: opts.reportingOptions?.aggregateOutput,
      throttleProgress: opts.reportingOptions?.throttleProgress,
      width: opts.reportingOptions?.outputMaxWidth,
      hideAddedPkgsProgress: opts.reportingOptions?.hideAddedPkgsProgress,
      hideProgressPrefix:
        opts.reportingOptions?.hideProgressPrefix ?? cmd === 'dlx',
      hideLifecycleOutput: opts.reportingOptions?.hideLifecycleOutput,
      hideLifecyclePrefix: opts.reportingOptions?.hideLifecyclePrefix,
    })

  if (opts.reportingOptions?.appendOnly) {
    return Rx.merge(...outputs).pipe(
      map((log: Rx.Observable<{ msg: string }>) =>
        log.pipe(map((msg) => msg.msg))
      ),
      mergeAll()
    )
  }
  return mergeOutputs(outputs)
}
