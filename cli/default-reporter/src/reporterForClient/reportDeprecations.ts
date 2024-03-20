import * as Rx from 'rxjs'
import chalk from 'chalk'
import { map, filter, buffer, switchMap } from 'rxjs/operators'

import { zoomOut } from './utils/zooming'
import { formatWarn } from './utils/formatWarn'

import { DeprecationLog, StageLog } from '@pnpm/types'

export function reportDeprecations(
  log$: {
    deprecation: Rx.Observable<DeprecationLog>
    stage: Rx.Observable<StageLog>
  },
  opts: {
    cwd: string
    isRecursive: boolean
  }
) {
  const [deprecatedDirectDeps$, deprecatedSubdeps$] = Rx.partition(
    log$.deprecation,
    (log) => log.depth === 0
  )
  const resolutionDone$ = log$.stage.pipe(
    filter((log) => log.stage === 'resolution_done')
  )
  return Rx.merge(
    deprecatedDirectDeps$.pipe(
      map((log) => {
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
      switchMap((deprecatedSubdeps) => {
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
