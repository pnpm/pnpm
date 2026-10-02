import type { SkippedOptionalDependencyLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { filter, map } from 'rxjs/operators'

import { formatWarn } from './utils/formatWarn.js'

/**
 * A package that could not be fetched is reported as a warning wherever it
 * sits in the graph. Any other skip is reported only for a direct optional
 * dependency of the current project.
 */
export function reportSkippedOptionalDependencies (
  skippedOptionalDependency$: Rx.Observable<SkippedOptionalDependencyLog>,
  opts: {
    cwd: string
  }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  return skippedOptionalDependency$.pipe(
    filter((log) => log['prefix'] === opts.cwd && (log.reason === 'fetch_failure' || log.parents?.length === 0)),
    map((log) => Rx.of({ msg: formatSkippedOptionalDependency(log) }))
  )
}

function formatSkippedOptionalDependency (log: SkippedOptionalDependencyLog): string {
  switch (log.reason) {
    case 'resolution_failure':
      return `info: ${
        log.package.name ? `${log.package.name}@${log.package.bareSpecifier}` : log.package.bareSpecifier
      } is an optional dependency that could not be resolved. Excluding it from installation.`
    case 'fetch_failure': {
      const msg = `${log.package.name}@${log.package.version} is an optional dependency that could not be fetched. Excluding it from installation.`
      return formatWarn(log.details ? `${msg}\n${log.details}` : msg)
    }
    default:
      return `info: ${log.package.id} is an optional dependency and failed compatibility check. Excluding it from installation.`
  }
}
