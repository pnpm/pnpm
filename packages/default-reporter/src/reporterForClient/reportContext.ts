import { ContextLog, PackageImportMethodLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { map, take } from 'rxjs/operators'
import path = require('path')

export default (
  log$: {
    context: Rx.Observable<ContextLog>
    packageImportMethod: Rx.Observable<PackageImportMethodLog>
  },
  opts: { cwd: string }
) => {
  return Rx.combineLatest(
    log$.context.pipe(take(1)),
    log$.packageImportMethod.pipe(take(1))
  )
    .pipe(
      map(
        ([context, packageImportMethod]) => {
          if (context.currentLockfileExists) {
            return Rx.NEVER
          }
          let method!: string
          switch (packageImportMethod.method) {
          case 'copy':
            method = 'copied'
            break
          case 'clone':
            method = 'cloned'
            break
          case 'hardlink':
            method = 'hard linked'
            break
          default:
            method = packageImportMethod.method
            break
          }
          return Rx.of({
            msg: `\
Packages are ${method} from the content-addressable store to the virtual store.
  Content-addressable store is at: ${context.storeDir}
  Virtual store is at:             ${path.relative(opts.cwd, context.virtualStoreDir)}`,
          })
        }
      )
    )
}
