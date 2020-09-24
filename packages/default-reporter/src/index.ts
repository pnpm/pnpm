import { Config } from '@pnpm/config'
import * as logs from '@pnpm/core-loggers'
import { LogLevel } from '@pnpm/logger'
import * as Rx from 'rxjs'
import { map, mergeAll } from 'rxjs/operators'
import { EOL } from './constants'
import mergeOutputs from './mergeOutputs'
import reporterForClient from './reporterForClient'
import reporterForServer from './reporterForServer'
import createDiffer = require('ansi-diff')

export default function (
  opts: {
    streamParser: object
    reportingOptions?: {
      appendOnly?: boolean
      logLevel?: LogLevel
      streamLifecycleOutput?: boolean
      throttleProgress?: number
      outputMaxWidth?: number
    }
    context: {
      argv: string[]
      config?: Config
    }
  }
) {
  if (opts.context.argv[0] === 'server') {
    // eslint-disable-next-line
    const log$ = Rx.fromEvent<logs.Log>(opts.streamParser as any, 'data')
    reporterForServer(log$, opts.context.config)
    return
  }
  const outputMaxWidth = opts.reportingOptions?.outputMaxWidth ?? (process.stdout.columns && process.stdout.columns - 2) ?? 80
  const output$ = toOutput$({ ...opts, reportingOptions: { ...opts.reportingOptions, outputMaxWidth } })
  if (opts.reportingOptions?.appendOnly) {
    output$
      .subscribe({
        complete () {}, // eslint-disable-line:no-empty
        error: (err) => console.error(err.message),
        next: (line) => console.log(line),
      })
    return
  }
  const diff = createDiffer({
    height: process.stdout.rows,
    outputMaxWidth,
  })
  output$
    .subscribe({
      complete () {}, // eslint-disable-line:no-empty
      error: (err) => logUpdate(err.message),
      next: logUpdate,
    })
  function logUpdate (view: string) {
    // A new line should always be appended in case a prompt needs to appear.
    // Without a new line the prompt will be joined with the previous output.
    // An example of such prompt may be seen by running: pnpm update --interactive
    if (!view.endsWith(EOL)) view += EOL
    process.stdout.write(diff.update(view))
  }
}

export function toOutput$ (
  opts: {
    streamParser: object
    reportingOptions?: {
      appendOnly?: boolean
      logLevel?: LogLevel
      outputMaxWidth?: number
      streamLifecycleOutput?: boolean
      throttleProgress?: number
    }
    context: {
      argv: string[]
      config?: Config
    }
  }
): Rx.Observable<string> {
  opts = opts || {}
  const contextPushStream = new Rx.Subject<logs.ContextLog>()
  const fetchingProgressPushStream = new Rx.Subject<logs.FetchingProgressLog>()
  const progressPushStream = new Rx.Subject<logs.ProgressLog>()
  const stagePushStream = new Rx.Subject<logs.StageLog>()
  const deprecationPushStream = new Rx.Subject<logs.DeprecationLog>()
  const summaryPushStream = new Rx.Subject<logs.SummaryLog>()
  const lifecyclePushStream = new Rx.Subject<logs.LifecycleLog>()
  const statsPushStream = new Rx.Subject<logs.StatsLog>()
  const packageImportMethodPushStream = new Rx.Subject<logs.PackageImportMethodLog>()
  const installCheckPushStream = new Rx.Subject<logs.InstallCheckLog>()
  const registryPushStream = new Rx.Subject<logs.RegistryLog>()
  const rootPushStream = new Rx.Subject<logs.RootLog>()
  const packageManifestPushStream = new Rx.Subject<logs.PackageManifestLog>()
  const linkPushStream = new Rx.Subject<logs.LinkLog>()
  const otherPushStream = new Rx.Subject<logs.Log>()
  const hookPushStream = new Rx.Subject<logs.HookLog>()
  const skippedOptionalDependencyPushStream = new Rx.Subject<logs.SkippedOptionalDependencyLog>()
  const scopePushStream = new Rx.Subject<logs.ScopeLog>()
  const requestRetryPushStream = new Rx.Subject<logs.RequestRetryLog>()
  setTimeout(() => {
    opts.streamParser['on']('data', (log: logs.Log) => {
      switch (log.name) {
      case 'pnpm:context':
        contextPushStream.next(log)
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
      case 'pnpm' as any: // eslint-disable-line
      case 'pnpm:global' as any: // eslint-disable-line
      case 'pnpm:store' as any: // eslint-disable-line
      case 'pnpm:lockfile' as any: // eslint-disable-line
        otherPushStream.next(log)
        break
      }
    })
  }, 0)
  const log$ = {
    context: Rx.from(contextPushStream),
    deprecation: Rx.from(deprecationPushStream),
    fetchingProgress: Rx.from(fetchingProgressPushStream),
    hook: Rx.from(hookPushStream),
    installCheck: Rx.from(installCheckPushStream),
    lifecycle: Rx.from(lifecyclePushStream),
    link: Rx.from(linkPushStream),
    other: Rx.from(otherPushStream),
    packageImportMethod: Rx.from(packageImportMethodPushStream),
    packageManifest: Rx.from(packageManifestPushStream),
    progress: Rx.from(progressPushStream),
    registry: Rx.from(registryPushStream),
    requestRetry: Rx.from(requestRetryPushStream),
    root: Rx.from(rootPushStream),
    scope: Rx.from(scopePushStream),
    skippedOptionalDependency: Rx.from(skippedOptionalDependencyPushStream),
    stage: Rx.from(stagePushStream),
    stats: Rx.from(statsPushStream),
    summary: Rx.from(summaryPushStream),
  }
  const outputs: Array<Rx.Observable<Rx.Observable<{msg: string}>>> = reporterForClient(
    log$,
    {
      appendOnly: opts.reportingOptions?.appendOnly,
      cmd: opts.context.argv[0],
      config: opts.context.config,
      isRecursive: opts.context.config?.['recursive'] === true,
      logLevel: opts.reportingOptions?.logLevel,
      pnpmConfig: opts.context.config,
      streamLifecycleOutput: opts.reportingOptions?.streamLifecycleOutput,
      throttleProgress: opts.reportingOptions?.throttleProgress,
      width: opts.reportingOptions?.outputMaxWidth,
    }
  )

  if (opts.reportingOptions?.appendOnly) {
    return Rx.merge(...outputs)
      .pipe(
        map((log: Rx.Observable<{msg: string}>) => log.pipe(map((msg) => msg.msg))),
        mergeAll()
      )
  }
  return mergeOutputs(outputs)
}
