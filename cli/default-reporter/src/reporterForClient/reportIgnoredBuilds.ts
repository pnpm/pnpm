import type { Config, ConfigContext } from '@pnpm/config.reader'
import type { IgnoredScriptsLog } from '@pnpm/core-loggers'
import { lexCompare } from '@pnpm/util.lex-comparator'
import boxen from 'boxen'
import * as Rx from 'rxjs'
import { map } from 'rxjs/operators'

export function reportIgnoredBuilds (
  log$: {
    ignoredScripts: Rx.Observable<IgnoredScriptsLog>
  },
  opts: {
    pnpmConfig?: Config & ConfigContext
    // This is used by Bit CLI
    approveBuildsInstructionText?: string
  }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  return log$.ignoredScripts.pipe(
    map((ignoredScripts) => {
      if (ignoredScripts.packageNames && ignoredScripts.packageNames.length > 0 && !opts.pnpmConfig?.strictDepBuilds) {
        const msg = boxen(`Ignored build scripts for: ${Array.from(ignoredScripts.packageNames).sort(lexCompare).join(', ')}.

Starting with pnpm v10, lifecycle scripts (like postinstall) are blocked by default for security to prevent supply chain attacks.
However, many packages (like sharp, esbuild, bcrypt, etc.) REQUIRE these scripts to build native binaries or download engines.
If you do not allow them, your application may fail at runtime with "Module not found" or similar errors.

${opts.approveBuildsInstructionText ?? `Run "pnpm approve-builds${opts.pnpmConfig?.cliOptions?.global ? ' -g' : ''}" to allow scripts for trusted packages.`}

For more information, see: https://pnpm.io/npm-scripts#onlybuiltdependencies`, {
          title: 'Security Warning',
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
