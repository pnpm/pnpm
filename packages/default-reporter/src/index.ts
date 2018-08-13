import {PnpmConfigs} from '@pnpm/config'
import createDiffer = require('ansi-diff')
import cliCursor = require('cli-cursor')
import most = require('most')
import * as supi from 'supi'
import PushStream = require('zen-push')
import {EOL} from './constants'
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
    const log$ = most.fromEvent<supi.Log>('data', opts.streamParser)
    reporterForServer(log$)
    return
  }
  const outputMaxWidth = opts.reportingOptions && opts.reportingOptions.outputMaxWidth || process.stdout.columns && process.stdout.columns - 2 || 80
  const output$ = toOutput$({...opts, reportingOptions: {...opts.reportingOptions, outputMaxWidth}})
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
  setTimeout(() => { // setTimeout is a workaround for a strange bug in most https://github.com/cujojs/most/issues/491
    opts.streamParser['on']('data', (log: supi.Log) => {
      switch (log.name) {
        case 'pnpm:progress':
          progressPushStream.next(log as supi.ProgressLog)
          break
        case 'pnpm:stage':
          stagePushStream.next(log as supi.StageLog)
          break
        case 'pnpm:deprecation':
          deprecationPushStream.next(log as supi.DeprecationLog)
          break
        case 'pnpm:summary':
          summaryPushStream.next(log)
          break
        case 'pnpm:lifecycle':
          lifecyclePushStream.next(log as supi.LifecycleLog)
          break
        case 'pnpm:stats':
          statsPushStream.next(log as supi.StatsLog)
          break
        case 'pnpm:install-check':
          installCheckPushStream.next(log as supi.InstallCheckLog)
          break
        case 'pnpm:registry':
          registryPushStream.next(log as supi.RegistryLog)
          break
        case 'pnpm:root':
          rootPushStream.next(log as supi.RootLog)
          break
        case 'pnpm:package-json':
          packageJsonPushStream.next(log as supi.PackageJsonLog)
          break
        case 'pnpm:link' as any: // tslint:disable-line
          linkPushStream.next(log)
          break
        case 'pnpm:cli' as any: // tslint:disable-line
          cliPushStream.next(log)
          break
        case 'pnpm:hook' as any: // tslint:disable-line
          hookPushStream.next(log)
          break
        case 'pnpm:skipped-optional-dependency':
          skippedOptionalDependencyPushStream.next(log as supi.SkippedOptionalDependencyLog)
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
    cli: most.from<supi.Log>(cliPushStream.observable),
    deprecation: most.from<supi.DeprecationLog>(deprecationPushStream.observable),
    hook: most.from<supi.Log>(hookPushStream.observable),
    installCheck: most.from<supi.InstallCheckLog>(installCheckPushStream.observable),
    lifecycle: most.from<supi.LifecycleLog>(lifecyclePushStream.observable),
    link: most.from<supi.Log>(linkPushStream.observable),
    other: most.from<supi.Log>(otherPushStream.observable),
    packageJson: most.from<supi.PackageJsonLog>(packageJsonPushStream.observable),
    progress: most.from<supi.ProgressLog>(progressPushStream.observable),
    registry: most.from<supi.RegistryLog>(registryPushStream.observable),
    root: most.from<supi.RootLog>(rootPushStream.observable),
    skippedOptionalDependency: most.from<supi.SkippedOptionalDependencyLog>(skippedOptionalDependencyPushStream.observable),
    stage: most.from<supi.StageLog>(stagePushStream.observable),
    stats: most.from<supi.StatsLog>(statsPushStream.observable),
    summary: most.from<supi.SummaryLog>(summaryPushStream.observable),
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
