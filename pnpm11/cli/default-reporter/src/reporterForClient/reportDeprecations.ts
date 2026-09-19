import type { DeprecationLog, StageLog } from '@pnpm/core-loggers'
import chalk from 'chalk'
import * as Rx from 'rxjs'
import { buffer, filter, map, switchMap } from 'rxjs/operators'

import { formatWarn } from './utils/formatWarn.js'
import { autozoom } from './utils/zooming.js'

export function reportDeprecations (
  log$: {
    deprecation: Rx.Observable<DeprecationLog>
    stage: Rx.Observable<StageLog>
  },
  opts: {
    cwd: string
    isRecursive: boolean
  }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  const [deprecatedDirectDeps$, deprecatedSubdeps$] = Rx.partition(log$.deprecation, (log) => log.depth === 0)
  const resolutionDone$ = log$.stage.pipe(
    filter((log) => log.stage === 'resolution_done')
  )
  return Rx.merge(
    deprecatedDirectDeps$.pipe(
      map((log) => {
        const pkg = `${log.pkgName}@${log.pkgVersion}`
        const line = formatWarn(`${chalk.red('deprecated')} ${pkg}. Run "pnpm view ${pkg}" to see why.`)
        return Rx.of({
          msg: autozoom(opts.cwd, log.prefix, line, { zoomOutCurrent: opts.isRecursive }),
        })
      })
    ),
    deprecatedSubdeps$.pipe(
      buffer(resolutionDone$),
      switchMap(deprecatedSubdeps => {
        if (deprecatedSubdeps.length > 0) {
          return Rx.of(Rx.of({
            msg: formatWarn(`${chalk.red(`${deprecatedSubdeps.length} deprecated subdependencies found:`)} ${deprecatedSubdeps.map(log => `${log.pkgName}@${log.pkgVersion}`).sort().join(', ')}`),
          }))
        }
        return Rx.EMPTY
      })
    )
  )
}
