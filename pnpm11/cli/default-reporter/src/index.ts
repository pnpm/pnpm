import type * as logs from '@pnpm/core-loggers'
import type { LogLevel, StreamParser } from '@pnpm/logger'
import createDiffer from 'ansi-diff'
import * as Rx from 'rxjs'
import { filter, map, mergeAll } from 'rxjs/operators'
import stringLength from 'string-length'

import { EOL } from './constants.js'
import { mergeOutputs } from './mergeOutputs.js'
import { reporterForClient } from './reporterForClient/index.js'
import type { FilterPkgsDiff } from './reporterForClient/reportSummary.js'
import { formatWarn } from './reporterForClient/utils/formatWarn.js'
import type { ReporterPnpmConfig } from './ReporterPnpmConfig.js'

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
      config?: ReporterPnpmConfig
      env?: NodeJS.ProcessEnv
      process?: NodeJS.Process
    }
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
  // The width the live differ wraps its frame at, so a resize can be noticed.
  let differWidth = 0
  const newDiffer = (): ReturnType<typeof createDiffer> => {
    differWidth = Math.max(1, stream.columns ?? outputMaxWidth)
    return createDiffer({ height: stream.rows, width: differWidth })
  }
  let diff = newDiffer()
  // How many leading lines of the view have scrolled out of the differ's frame
  // and been committed to the scrollback, how many rows the frame the differ is
  // holding takes up, and whether it already outgrew the terminal it was drawn
  // on. See `commitOverflow`.
  let committedLines = 0
  let renderedFrameRows = 0
  let renderedFrameOutgrewTerminal = false
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
    const lines = view.slice(0, -EOL.length).split(EOL)
    const committed = commitOverflow(lines)
    // The lines from `committedLines` on are already laid out contiguously in
    // the view, so the visible frame is a slice of it rather than a second copy.
    const frame = view.slice(viewOffsetOfLine(view, lines, committedLines))
    // `\r` resets the column to 0 in case an external process (e.g. an SSH
    // passphrase prompt) left the cursor mid-line. `ansi-diff` then writes
    // only the differential — the characters that actually changed between
    // the previous frame and this one — so sticky blocks like the lockfile
    // verdict and deprecation warnings are not re-written on every progress
    // tick. `\x1b[K` erases trailing characters on the current line;
    // `\x1b[0J` erases anything an external process wrote below the
    // rendered frame.
    write(`\r${committed}${diff.update(frame)}\x1b[K${ERASE_TO_END_OF_DISPLAY}`)
  }
  /**
   * Hands the lines that no longer fit on screen over to the scrollback and
   * restarts the differ below them, returning the differential that performs
   * the handover.
   *
   * `ansi-diff` redraws by moving the cursor up from the end of its frame, so
   * it can only reach lines that are still on screen. A frame taller than the
   * terminal has scrolled its top away, and every later redraw then lands that
   * many rows too low — overwriting output above the frame instead of updating
   * it (pnpm/pnpm#14270). Committing the overflow keeps the frame within the
   * terminal, at the cost of no longer being able to revise what was committed.
   */
  function commitOverflow (lines: string[]): string {
    if (Math.max(1, stream.columns ?? outputMaxWidth) !== differWidth) {
      // The terminal was resized. The frame on screen has reflowed at the new
      // width, so every position the differ tracked against the old one is
      // wrong: start over below what is already there.
      diff = newDiffer()
    }
    if (lines.length <= committedLines) {
      // The view no longer reaches past what was committed — an error frame
      // replaces it rather than extending it. Render it whole, below.
      committedLines = 0
      diff = newDiffer()
      return ''
    }
    const rows = stream.rows
    if (!rows) return ''
    const width = differWidth
    // One row is left over for the cursor line that the trailing EOL puts
    // below the frame.
    const maxRows = Math.max(rows - 1, 1)
    let uncommittedRows = 0
    for (let i = committedLines; i < lines.length; i++) {
      uncommittedRows += renderedRows(lines[i], width)
    }
    // The last line always stays in the frame — there would be nothing left to
    // redraw otherwise — so the walk upwards starts one line above it.
    let firstVisible = lines.length - 1
    let frameRows = renderedRows(lines[firstVisible], width)
    for (let i = firstVisible - 1; i >= committedLines; i--) {
      const lineRows = renderedRows(lines[i], width)
      if (frameRows + lineRows > maxRows) break
      frameRows += lineRows
      firstVisible = i
    }
    // A frame taller than the terminal has scrolled its own top away — whether
    // because a line outgrew the screen or because the window shrank under it —
    // so no cursor move reaches back into it, and growing the window again does
    // not bring it back. Start afresh below instead, reprinting rather than
    // revising, and leave the commit for the next frame, whose layout is one
    // this differ laid out itself.
    const cannotRevise = renderedFrameOutgrewTerminal || renderedFrameRows > maxRows
    if (cannotRevise || firstVisible === committedLines) {
      renderedFrameRows = uncommittedRows
      renderedFrameOutgrewTerminal = uncommittedRows > maxRows
      if (cannotRevise || renderedFrameOutgrewTerminal) diff = newDiffer()
      return ''
    }
    renderedFrameRows = frameRows
    renderedFrameOutgrewTerminal = false
    // Shrinking the frame to just the overflow leaves those lines untouched
    // where they already are, erases the rest of the frame below them, and
    // parks the cursor on the next line — where the fresh differ starts.
    const handover = diff.update(`${lines.slice(committedLines, firstVisible).join(EOL)}${EOL}`).toString()
    diff = newDiffer()
    committedLines = firstVisible
    return handover
  }
  return () => {
    subscription.unsubscribe()
    opts.streamParser.removeListener('data', onLog)
  }
}

/**
 * Where the `index`-th of `lines` starts in the `view` they were split from.
 * Measured from the end, so a long committed prefix costs nothing.
 */
function viewOffsetOfLine (view: string, lines: string[], index: number): number {
  let trailing = 0
  for (let i = lines.length - 1; i >= index; i--) {
    trailing += lines[i].length + EOL.length
  }
  return view.length - trailing
}

/**
 * How many terminal rows `line` occupies once wrapped at `width`, counting the
 * escape sequences in it as zero-width. Never zero: an empty line still takes a
 * row. `width` is the terminal's own column count, clamped to at least one.
 */
function renderedRows (line: string, width: number): number {
  return Math.max(1, Math.ceil(stringLength(line) / width))
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
      config?: ReporterPnpmConfig
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
