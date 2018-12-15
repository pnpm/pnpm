import { PnpmConfigs } from '@pnpm/config'
import * as logs from '@pnpm/core-loggers'
import createDiffer = require('ansi-diff')
import cliCursor = require('cli-cursor')
import most = require('most')
import PushStream = require('zen-push')
import { EOL } from './constants'
import mergeOutputs from './mergeOutputs'
import reporterForClient from './reporterForClient'
import reporterForServer from './reporterForServer'

export default function (
  opts: {
    streamParser: object,
    reportingOptions?: {
      appendOnly?: boolean,
      throttleProgress?: number,
      outputMaxWidth?: number,
    },
    context: {
      argv: string[],
      configs?: PnpmConfigs,
    },
  },
) {
  if (opts.context.argv[0] === 'server') {
    const log$ = most.fromEvent<logs.Log>('data', opts.streamParser)
    reporterForServer(log$)
    return
  }
  const outputMaxWidth = opts.reportingOptions && opts.reportingOptions.outputMaxWidth || process.stdout.columns && process.stdout.columns - 2 || 80
  const output$ = toOutput$({ ...opts, reportingOptions: { ...opts.reportingOptions, outputMaxWidth } })
  if (opts.reportingOptions && opts.reportingOptions.appendOnly) {
    output$
      .subscribe({
        complete () {}, // tslint:disable-line:no-empty
        error: (err) => console.error(err.message),
        next: (line) => console.log(line),
      })
    return
  }
  cliCursor.hide()
  const diff = createDiffer({
    height: process.stdout.rows,
    outputMaxWidth,
  })
  output$
    .subscribe({
      complete () {}, // tslint:disable-line:no-empty
      error: (err) => logUpdate(err.message),
      next: logUpdate,
    })
  function logUpdate (view: string) {
    process.stdout.write(diff.update(`${view}${EOL}`))
  }
}

export function toOutput$ (
  opts: {
    streamParser: object,
    reportingOptions?: {
      appendOnly?: boolean,
      throttleProgress?: number,
      outputMaxWidth?: number,
    },
    context: {
      argv: string[],
      configs?: PnpmConfigs,
    },
  },
): most.Stream<string> {
  opts = opts || {}
  const progressPushStream = new PushStream()
  const stagePushStream = new PushStream()
  const deprecationPushStream = new PushStream()
  const summaryPushStream = new PushStream()
  const lifecyclePushStream = new PushStream()
  const statsPushStream = new PushStream()
  const installCheckPushStream = new PushStream()
  const registryPushStream = new PushStream()
  const rootPushStream = new PushStream()
  const packageJsonPushStream = new PushStream()
  const linkPushStream = new PushStream()
  const cliPushStream = new PushStream()
  const otherPushStream = new PushStream()
  const hookPushStream = new PushStream()
  const skippedOptionalDependencyPushStream = new PushStream()
  const scopePushStream = new PushStream()
  setTimeout(() => { // setTimeout is a workaround for a strange bug in most https://github.com/cujojs/most/issues/491
    opts.streamParser['on']('data', (log: logs.Log) => {
      switch (log.name) {
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
        case 'pnpm:install-check':
          installCheckPushStream.next(log)
          break
        case 'pnpm:registry':
          registryPushStream.next(log)
          break
        case 'pnpm:root':
          rootPushStream.next(log)
          break
        case 'pnpm:package-json':
          packageJsonPushStream.next(log)
          break
        case 'pnpm:link':
          linkPushStream.next(log)
          break
        case 'pnpm:cli':
          cliPushStream.next(log)
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
        case 'pnpm' as any: // tslint:disable-line
        case 'pnpm:store' as any: // tslint:disable-line
        case 'pnpm:shrinkwrap' as any: // tslint:disable-line
          otherPushStream.next(log)
          break
      }
    })
  }, 0)
  const log$ = {
    cli: most.from<logs.CliLog>(cliPushStream.observable),
    deprecation: most.from<logs.DeprecationLog>(deprecationPushStream.observable),
    hook: most.from<logs.HookLog>(hookPushStream.observable),
    installCheck: most.from<logs.InstallCheckLog>(installCheckPushStream.observable),
    lifecycle: most.from<logs.LifecycleLog>(lifecyclePushStream.observable),
    link: most.from<logs.LinkLog>(linkPushStream.observable),
    other: most.from<logs.Log>(otherPushStream.observable),
    packageJson: most.from<logs.PackageJsonLog>(packageJsonPushStream.observable),
    progress: most.from<logs.ProgressLog>(progressPushStream.observable),
    registry: most.from<logs.RegistryLog>(registryPushStream.observable),
    root: most.from<logs.RootLog>(rootPushStream.observable),
    scope: most.from<logs.ScopeLog>(scopePushStream.observable),
    skippedOptionalDependency: most.from<logs.SkippedOptionalDependencyLog>(skippedOptionalDependencyPushStream.observable),
    stage: most.from<logs.StageLog>(stagePushStream.observable),
    stats: most.from<logs.StatsLog>(statsPushStream.observable),
    summary: most.from<logs.SummaryLog>(summaryPushStream.observable),
  }
  const outputs: Array<most.Stream<most.Stream<{msg: string}>>> = reporterForClient(
    log$,
    {
      appendOnly: opts.reportingOptions && opts.reportingOptions.appendOnly,
      cmd: opts.context.argv[0],
      isRecursive: opts.context.argv[0] === 'recursive',
      pnpmConfigs: opts.context.configs,
      subCmd: opts.context.argv[1],
      throttleProgress: opts.reportingOptions && opts.reportingOptions.throttleProgress,
      width: opts.reportingOptions && opts.reportingOptions.outputMaxWidth,
    },
  )

  if (opts.reportingOptions && opts.reportingOptions.appendOnly) {
    return most.join(
      most.mergeArray(outputs)
      .map((log: most.Stream<{msg: string}>) => log.map((msg) => msg.msg)),
    )
  }
  return mergeOutputs(outputs).multicast()
}
