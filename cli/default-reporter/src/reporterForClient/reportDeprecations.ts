import chalk from 'chalk'
import * as Rx from 'rxjs'
import { map, filter, buffer, switchMap } from 'rxjs/operators'

import { zoomOut } from './utils/zooming.js'
import { formatWarn } from './utils/formatWarn.js'

import type { DeprecationLog, StageLog } from '@pnpm/types'

export function reportDeprecations(
  log$: {
    deprecation: Rx.Observable<DeprecationLog>
    stage: Rx.Observable<StageLog>
  },
  opts: {
    cwd: string
    isRecursive: boolean
  }
): Rx.Observable<Rx.Observable<{
  msg: string;
}> | Rx.Observable<{
  msg: string;
}>> {
  const [deprecatedDirectDeps$, deprecatedSubdeps$] = Rx.partition(
    log$.deprecation,
    (log: DeprecationLog): boolean => {
      return log.depth === 0;
    }
  )

  const resolutionDone$ = log$.stage.pipe(
    filter((log: StageLog): boolean => {
      return log.stage === 'resolution_done';
    })
  )

  return Rx.merge(
    deprecatedDirectDeps$.pipe(
      map((log: DeprecationLog): Rx.Observable<{
        msg: string;
      }> => {
        if (!opts.isRecursive && log.prefix === opts.cwd) {
          return Rx.of({
            msg: formatWarn(
              `${chalk.red('deprecated')} ${log.pkgName}@${log.pkgVersion}: ${log.deprecated}`
            ),
          })
        }

        return Rx.of({
          msg: zoomOut(
            opts.cwd,
            log.prefix,
            formatWarn(
              `${chalk.red('deprecated')} ${log.pkgName}@${log.pkgVersion}`
            )
          ),
        })
      })
    ),
    deprecatedSubdeps$.pipe(
      buffer(resolutionDone$),
      switchMap((deprecatedSubdeps: DeprecationLog[]): Rx.Observable<Rx.Observable<{
        msg: string;
      }>> => {
        if (deprecatedSubdeps.length > 0) {
          return Rx.of(
            Rx.of({
              msg: formatWarn(
                `${chalk.red(`${deprecatedSubdeps.length} deprecated subdependencies found:`)} ${deprecatedSubdeps
                  .map((log) => `${log.pkgName}@${log.pkgVersion}`)
                  .sort()
                  .join(', ')}`
              ),
            })
          )
        }

        return Rx.EMPTY
      })
    )
  )
}
