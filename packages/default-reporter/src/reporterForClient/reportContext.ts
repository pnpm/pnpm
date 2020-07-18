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
        return ({msg:  `${context.storeDir}, ${context.virtualStoreDir}, ${context.currentLockfileExists}, ${packageImportMethod.method}`})
      },
      log$.context,
      log$.packageImportMethod,
    )
    .take(1)
    .map(most.of)
}
