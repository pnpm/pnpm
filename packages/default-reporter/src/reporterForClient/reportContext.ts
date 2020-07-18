import { ContextLog, PackageImportMethodLog } from '@pnpm/core-loggers'
import most = require('most')

export default (
  log$: {
    context: most.Stream<ContextLog>,
    packageImportMethod: most.Stream<PackageImportMethodLog>,
  }
) => {
  return most.combine(
      (context, packageImportMethod) => {
        let method = 'hard linked'
        switch (packageImportMethod.method) {
          case 'copy':
            method = 'copied'
            break
          case 'clone':
            method = 'cloned'
            break
          default:
            break
        }
        return ({ msg: !context.currentLockfileExists ? `Packages were ${method} from the content-addressable store to the virtual store.\nContent-addressable store is at: ${context.storeDir}\nVirtual store is at: ${context.virtualStoreDir}` : '' })
      },
      log$.context,
      log$.packageImportMethod
    )
    .take(1)
    .map(most.of)
}
