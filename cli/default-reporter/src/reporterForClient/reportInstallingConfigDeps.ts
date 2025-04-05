import { type InstallingConfigDepsLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { map } from 'rxjs/operators'

export function reportInstallingConfigDeps (
  installingConfigDeps$: Rx.Observable<InstallingConfigDepsLog>
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  return Rx.of(installingConfigDeps$.pipe(
    map((log) => {
      switch (log.status) {
      case 'started': {
        return {
          msg: 'Installing config dependencies...',
        }
      }
      case 'done':
        return {
          msg: `Installed config dependencies: ${log.deps.map(({ name, version }) => `${name}@${version}`).join(', ')}`,
        }
      }
    })
  ))
}
