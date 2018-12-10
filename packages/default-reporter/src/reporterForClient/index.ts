import { PnpmConfigs } from '@pnpm/config'
import most = require('most')
import * as supi from 'supi'
import reportBigTarballsProgress from './reportBigTarballsProgress'
import reportDeprecations from './reportDeprecations'
import reportHooks from './reportHooks'
import reportInstallChecks from './reportInstallChecks'
import reportLifecycleScripts from './reportLifecycleScripts'
import reportMisc from './reportMisc'
import reportProgress from './reportProgress'
import reportSkippedOptionalDependencies from './reportSkippedOptionalDependencies'
import reportStats from './reportStats'
import reportSummary from './reportSummary'

export default function (
  log$: {
    progress: most.Stream<supi.ProgressLog>,
    stage: most.Stream<supi.StageLog>,
    deprecation: most.Stream<supi.DeprecationLog>,
    summary: most.Stream<supi.SummaryLog>,
    lifecycle: most.Stream<supi.LifecycleLog>,
    stats: most.Stream<supi.StatsLog>,
    installCheck: most.Stream<supi.InstallCheckLog>,
    registry: most.Stream<supi.RegistryLog>,
    root: most.Stream<supi.RootLog>,
    packageJson: most.Stream<supi.PackageJsonLog>,
    link: most.Stream<supi.Log>,
    other: most.Stream<supi.Log>,
    cli: most.Stream<supi.Log>,
    hook: most.Stream<supi.Log>,
    skippedOptionalDependency: most.Stream<supi.SkippedOptionalDependencyLog>,
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
