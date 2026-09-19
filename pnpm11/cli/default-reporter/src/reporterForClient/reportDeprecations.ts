import type { DeprecationLog, StageLog } from '@pnpm/core-loggers'
import { sanitizeInline } from '@pnpm/text.sanitize'
import chalk from 'chalk'
import * as Rx from 'rxjs'
import { buffer, filter, map, switchMap } from 'rxjs/operators'

import { formatWarn } from './utils/formatWarn.js'
import { autozoom } from './utils/zooming.js'

/**
 * `name@version` as the warning prints it.
 *
 * Both halves come from the resolved manifest, which a git or tarball
 * dependency writes itself, so neither is a validated npm package name.
 */
function pkgLabel (log: DeprecationLog): string {
  return sanitizeInline(`${log.pkgName}@${log.pkgVersion}`)
}

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
        const line = formatWarn(`${chalk.red('deprecated')} ${pkgLabel(log)}`)
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
            msg: formatWarn(`${chalk.red(`${deprecatedSubdeps.length} deprecated subdependencies found:`)} ${deprecatedSubdeps.map(pkgLabel).sort().join(', ')}`),
          }))
        }
        return Rx.EMPTY
      })
    )
  )
}
