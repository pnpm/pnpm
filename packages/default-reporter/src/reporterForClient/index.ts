import { PnpmConfigs } from '@pnpm/config'
import * as logs from '@pnpm/core-loggers'
import most = require('most')
import reportBigTarballsProgress from './reportBigTarballsProgress'
import reportDeprecations from './reportDeprecations'
import reportHooks from './reportHooks'
import reportInstallChecks from './reportInstallChecks'
import reportLifecycleScripts from './reportLifecycleScripts'
import reportMisc from './reportMisc'
import reportProgress from './reportProgress'
import reportScope from './reportScope'
import reportSkippedOptionalDependencies from './reportSkippedOptionalDependencies'
import reportStats from './reportStats'
import reportSummary from './reportSummary'

export default function (
  log$: {
    progress: most.Stream<logs.ProgressLog>,
    stage: most.Stream<logs.StageLog>,
    deprecation: most.Stream<logs.DeprecationLog>,
    summary: most.Stream<logs.SummaryLog>,
    lifecycle: most.Stream<logs.LifecycleLog>,
    stats: most.Stream<logs.StatsLog>,
    installCheck: most.Stream<logs.InstallCheckLog>,
    registry: most.Stream<logs.RegistryLog>,
    root: most.Stream<logs.RootLog>,
    packageJson: most.Stream<logs.PackageJsonLog>,
    link: most.Stream<logs.LinkLog>,
    other: most.Stream<logs.Log>,
    cli: most.Stream<logs.CliLog>,
    hook: most.Stream<logs.HookLog>,
    scope: most.Stream<logs.ScopeLog>,
    skippedOptionalDependency: most.Stream<logs.SkippedOptionalDependencyLog>,
  },
  opts: {
    isRecursive: boolean,
    cmd: string,
    subCmd?: string,
    width?: number,
    appendOnly?: boolean,
    throttleProgress?: number,
    pnpmConfigs?: PnpmConfigs,
  },
): Array<most.Stream<most.Stream<{msg: string}>>> {
  const width = opts.width || process.stdout.columns || 80
  const cwd = opts.pnpmConfigs && opts.pnpmConfigs.prefix || process.cwd()

  const outputs: Array<most.Stream<most.Stream<{msg: string}>>> = [
    reportProgress(log$, opts),
    reportLifecycleScripts(log$, {
      appendOnly: opts.appendOnly,
      cwd,
      width,
    }),
    reportDeprecations(log$.deprecation, { cwd, isRecursive: opts.isRecursive }),
    reportMisc(
      log$,
      {
        cwd,
        zoomOutCurrent: opts.isRecursive,
      },
    ),
    ...reportStats(log$, {
      cmd: opts.cmd,
      cwd,
      isRecursive: opts.isRecursive,
      subCmd: opts.subCmd,
      width,
    }),
    reportInstallChecks(log$.installCheck, { cwd }),
    reportScope(log$.scope, { isRecursive: opts.isRecursive, cmd: opts.cmd, subCmd: opts.subCmd }),
    reportSkippedOptionalDependencies(log$.skippedOptionalDependency, { cwd }),
    reportHooks(log$.hook, { cwd, isRecursive: opts.isRecursive }),
  ]

  if (!opts.appendOnly) {
    outputs.push(reportBigTarballsProgress(log$))
  }

  if (!opts.isRecursive) {
    outputs.push(reportSummary(log$, {
      cwd,
      pnpmConfigs: opts.pnpmConfigs,
    }))
  }

  return outputs
}
