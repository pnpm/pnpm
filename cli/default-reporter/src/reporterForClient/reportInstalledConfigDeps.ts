import { type InstalledConfigDepsLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { map } from 'rxjs/operators'

export function reportInstalledConfigDeps (
  installedConfigDeps$: Rx.Observable<InstalledConfigDepsLog>
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  return installedConfigDeps$.pipe(
    map((log) => {
      if (log.deps.length === 0) return Rx.NEVER
      return Rx.of({
        msg: `The next config dependencies were installed: ${log.deps.map(({ name, version }) => `${name}@${version}`).join(', ')}`,
      })
    })
  )
}
