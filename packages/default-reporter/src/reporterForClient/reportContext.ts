import { ContextLog, PackageImportMethodLog } from '@pnpm/core-loggers'
import path = require('path')
import most = require('most')

export default (
  log$: {
    context: most.Stream<ContextLog>,
    packageImportMethod: most.Stream<PackageImportMethodLog>,
  },
  opts: { cwd: string }
) => {
  return most.combine(
    (context, packageImportMethod) => {
      if (context.currentLockfileExists) {
        return most.never()
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
      return most.of({
        msg: `\
Packages are ${method} from the content-addressable store to the virtual store.
  Content-addressable store is at: ${context.storeDir}
  Virtual store is at:             ${path.relative(opts.cwd, context.virtualStoreDir)}`,
      })
    },
    log$.context.take(1),
    log$.packageImportMethod.take(1)
  )
}
