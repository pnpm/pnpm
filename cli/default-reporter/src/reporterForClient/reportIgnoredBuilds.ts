import { type Config } from '@pnpm/config'
import { type IgnoredScriptsLog } from '@pnpm/core-loggers'
import { lexCompare } from '@pnpm/util.lex-comparator'
import * as Rx from 'rxjs'
import { map } from 'rxjs/operators'
import boxen from 'boxen'

export function reportIgnoredBuilds (
  log$: {
    ignoredScripts: Rx.Observable<IgnoredScriptsLog>
  },
  opts: {
    pnpmConfig?: Config
    // This is used by Bit CLI
    approveBuildsInstructionText?: string
  }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  return log$.ignoredScripts.pipe(
    map((ignoredScripts) => {
      if (ignoredScripts.packageNames && ignoredScripts.packageNames.length > 0 && !opts.pnpmConfig?.strictDepBuilds) {
        const msg = boxen(`Ignored build scripts: ${Array.from(ignoredScripts.packageNames).sort(lexCompare).join(', ')}.
${opts.approveBuildsInstructionText ?? `Run "pnpm approve-builds${opts.pnpmConfig?.cliOptions?.global ? ' -g' : ''}" to pick which dependencies should be allowed to run scripts.`}`, {
          title: 'Warning',
          padding: 1,
          margin: 0,
          borderStyle: 'round',
          borderColor: 'yellow',
        })
        return Rx.of({ msg })
      }
      return Rx.NEVER
    })
  )
}
