import type { Config, ConfigContext } from '@pnpm/config.reader'
import type * as logs from '@pnpm/core-loggers'
import type { LogLevel, StreamParser } from '@pnpm/logger'
import createDiffer from 'ansi-diff'
import * as Rx from 'rxjs'
import { filter, map, mergeAll } from 'rxjs/operators'

import { EOL } from './constants.js'
import { mergeOutputs } from './mergeOutputs.js'
import { reporterForClient } from './reporterForClient/index.js'
import type { FilterPkgsDiff } from './reporterForClient/reportSummary.js'
import { formatWarn } from './reporterForClient/utils/formatWarn.js'

export { formatWarn }

// ANSI "erase from cursor to end of display". Appended after each
// differential update so that anything an external process (e.g. an SSH
// passphrase prompt) wrote below the rendered frame is cleared.
const ERASE_TO_END_OF_DISPLAY = '\x1b[0J'

export function initDefaultReporter (
  opts: {
    useStderr?: boolean
    streamParser: StreamParser<logs.Log>
    reportingOptions?: {
      appendOnly?: boolean
      logLevel?: LogLevel
      streamLifecycleOutput?: boolean
      aggregateOutput?: boolean
      throttleProgress?: number
      outputMaxWidth?: number
      hideAddedPkgsProgress?: boolean
      hideProgressPrefix?: boolean
      hideLifecycleOutput?: boolean
      hideLifecyclePrefix?: boolean
      // This is used by Bit CLI
      approveBuildsInstructionText?: string
    }
    context: {
      argv: string[]
      config?: Config & ConfigContext
      env?: NodeJS.ProcessEnv
      process?: NodeJS.Process
    }
    filterPkgsDiff?: FilterPkgsDiff
  }
): () => void {
  const proc = opts.context.process ?? process
  const outputMaxWidth = opts.reportingOptions?.outputMaxWidth ?? (proc.stdout.columns && proc.stdout.columns - 2) ?? 80
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
    const subscription = output$
      .subscribe({
        complete () {}, // eslint-disable-line:no-empty
        error: (err) => {
          console.error(err.message)
        },
        next: writeNext,
      })
    return () => {
      subscription.unsubscribe()
    }
  }
  const stream = opts.useStderr ? proc.stderr : proc.stdout
  const write = stream.write.bind(stream)
  const newDiffer = (): ReturnType<typeof createDiffer> => createDiffer({
    height: stream.rows,
    width: stream.columns ?? outputMaxWidth,
  })
  let diff = newDiffer()
  // Hold redraws while an interactive prompt owns the terminal (see PromptMessage).
  let promptActive = false
  const onLog = (log: logs.Log): void => {
    if (log.name !== 'pnpm:prompt') return
    if ((log as logs.PromptLog).action === 'start') {
      promptActive = true
    } else {
      promptActive = false
      // Drop the differ's now-stale frame: the terminal below it changed while paused.
      diff = newDiffer()
    }
  }
  opts.streamParser.on('data', onLog)
  const subscription = output$
    .subscribe({
      complete () {}, // eslint-disable-line:no-empty
      error: (err) => {
        logUpdate(err.message)
      },
      next: logUpdate,
    })
  function logUpdate (view: string) {
    if (promptActive) return
    // A new line should always be appended in case a prompt needs to appear.
    // Without a new line the prompt will be joined with the previous output.
    // An example of such prompt may be seen by running: pnpm update --interactive
    if (!view.endsWith(EOL)) view += EOL
    // `\r` resets the column to 0 in case an external process (e.g. an SSH
    // passphrase prompt) left the cursor mid-line. `ansi-diff` then writes
    // only the differential — the characters that actually changed between
    // the previous frame and this one — so sticky blocks like the lockfile
    // verdict and deprecation warnings are not re-written on every progress
    // tick. `\x1b[K` erases trailing characters on the current line;
    // `\x1b[0J` erases anything an external process wrote below the
    // rendered frame.
    write(`\r${diff.update(view)}\x1b[K${ERASE_TO_END_OF_DISPLAY}`)
  }
  return () => {
    subscription.unsubscribe()
    opts.streamParser.removeListener('data', onLog)
  }
}

export function toOutput$ (
  opts: {
    streamParser: StreamParser<logs.Log>
    reportingOptions?: {
      appendOnly?: boolean
      logLevel?: LogLevel
      outputMaxWidth?: number
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
    context: {
      argv: string[]
      config?: Config & ConfigContext
      env?: NodeJS.ProcessEnv
      process?: NodeJS.Process
    }
    filterPkgsDiff?: FilterPkgsDiff
  }
): Rx.Observable<string> {
  opts = opts || {}
  const contextPushStream = new Rx.Subject<logs.ContextLog>()
  const fetchingProgressPushStream = new Rx.Subject<logs.FetchingProgressLog>()
  const executionTimePushStream = new Rx.Subject<logs.ExecutionTimeLog>()
  const progressPushStream = new Rx.Subject<logs.ProgressLog>()
  const stagePushStream = new Rx.Subject<logs.StageLog>()
  const deprecationPushStream = new Rx.Subject<logs.DeprecationLog>()
  const summaryPushStream = new Rx.Subject<logs.SummaryLog>()
  const lifecyclePushStream = new Rx.Subject<logs.LifecycleLog>()
  const lockfileVerificationPushStream = new Rx.Subject<logs.LockfileVerificationLog>()
  const statsPushStream = new Rx.Subject<logs.StatsLog>()
  const packageImportMethodPushStream = new Rx.Subject<logs.PackageImportMethodLog>()
  const installCheckPushStream = new Rx.Subject<logs.InstallCheckLog>()
  const installingConfigDepsStream = new Rx.Subject<logs.InstallingConfigDepsLog>()
  const ignoredScriptsPushStream = new Rx.Subject<logs.IgnoredScriptsLog>()
  const registryPushStream = new Rx.Subject<logs.RegistryLog>()
  const rootPushStream = new Rx.Subject<logs.RootLog>()
  const packageManifestPushStream = new Rx.Subject<logs.PackageManifestLog>()
  const peerDependencyIssuesPushStream = new Rx.Subject<logs.PeerDependencyIssuesLog>()
  const linkPushStream = new Rx.Subject<logs.LinkLog>()
  const otherPushStream = new Rx.Subject<logs.Log>()
  const hookPushStream = new Rx.Subject<logs.HookLog>()
  const skippedOptionalDependencyPushStream = new Rx.Subject<logs.SkippedOptionalDependencyLog>()
  const scopePushStream = new Rx.Subject<logs.ScopeLog>()
  const requestRetryPushStream = new Rx.Subject<logs.RequestRetryLog>()
  const updateCheckPushStream = new Rx.Subject<logs.UpdateCheckLog>()
  const unusedOverridePushStream = new Rx.Subject<logs.UnusedOverrideLog>()
  setTimeout(() => {
    opts.streamParser.on('data', (log: logs.Log) => {
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
        case 'pnpm:lockfile-verification':
          lockfileVerificationPushStream.next(log)
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
        case 'pnpm:installing-config-deps':
          installingConfigDepsStream.next(log)
          break
        case 'pnpm:ignored-scripts':
          ignoredScriptsPushStream.next(log)
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
        case 'pnpm:unused-override':
          unusedOverridePushStream.next(log)
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
    const filterFn = filterLogs.length === 1
      ? filterLogs[0]
      : (log: logs.Log) => filterLogs.every!((filterLog) => filterLog(log))
    other = other.pipe(filter(filterFn))
  }
  const log$ = {
    context: Rx.from(contextPushStream),
    deprecation: Rx.from(deprecationPushStream),
    fetchingProgress: Rx.from(fetchingProgressPushStream),
    executionTime: Rx.from(executionTimePushStream),
    hook: Rx.from(hookPushStream),
    installCheck: Rx.from(installCheckPushStream),
    installingConfigDeps: Rx.from(installingConfigDepsStream),
    ignoredScripts: Rx.from(ignoredScriptsPushStream),
    lifecycle: Rx.from(lifecyclePushStream),
    link: Rx.from(linkPushStream),
    lockfileVerification: Rx.from(lockfileVerificationPushStream),
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
    unusedOverride: Rx.from(unusedOverridePushStream),
  }
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
