import { SkippedOptionalDependencyLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { filter, map } from 'rxjs/operators'

export default (
  skippedOptionalDependency$: Rx.Observable<SkippedOptionalDependencyLog>,
  opts: {
    cwd: string
  }
) => {
  return skippedOptionalDependency$.pipe(
    filter((log) => Boolean(log['prefix'] === opts.cwd && log.parents && log.parents.length === 0)),
    map((log) => Rx.of({
      msg: `info: ${
        // eslint-disable-next-line @typescript-eslint/restrict-template-expressions
        log.package['id'] || log.package.name && (`${log.package.name}@${log.package.version}`) || log.package['pref']
      } is an optional dependency and failed compatibility check. Excluding it from installation.`,
    }))
  )
}
