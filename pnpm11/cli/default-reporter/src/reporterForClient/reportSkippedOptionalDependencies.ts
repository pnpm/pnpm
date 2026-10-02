import type { SkippedOptionalDependencyLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { filter, map } from 'rxjs/operators'

import { formatWarn } from './utils/formatWarn.js'

/**
 * Reports a skipped direct optional dependency of the current project. A
 * package that could not be fetched is reported by
 * {@link reportOptionalFetchFailures} instead.
 */
export function reportSkippedOptionalDependencies (
  skippedOptionalDependency$: Rx.Observable<SkippedOptionalDependencyLog>,
  opts: {
    cwd: string
  }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  return skippedOptionalDependency$.pipe(
    filter((log) => log.reason !== 'fetch_failure' && log['prefix'] === opts.cwd && log.parents?.length === 0),
    map((log) => Rx.of({
      msg: log.reason === 'resolution_failure'
        ? `info: ${
          log.package.name ? `${log.package.name}@${log.package.bareSpecifier}` : log.package.bareSpecifier
        } is an optional dependency that could not be resolved. Excluding it from installation.`
        : `info: ${log.package.id} is an optional dependency and failed compatibility check. Excluding it from installation.`,
    }))
  )
}

/**
 * Warns about an optional package that could not be fetched, wherever it sits
 * in the graph and whichever project the install runs from, since it is
 * missing for the whole install.
 */
export function reportOptionalFetchFailures (
  skippedOptionalDependency$: Rx.Observable<SkippedOptionalDependencyLog>
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  return skippedOptionalDependency$.pipe(
    filter((log) => log.reason === 'fetch_failure'),
    map((log) => {
      const msg = `${log.package.name}@${log.package.version} is an optional dependency that could not be fetched. Excluding it from installation.`
      return Rx.of({ msg: formatWarn(log.details ? `${msg}\n${log.details}` : msg) })
    })
  )
}
